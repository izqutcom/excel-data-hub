use crate::models::{ExcelData, ImportStats, SearchResponse, StatsResponse};
use crate::models::entity::{excel_data, files};
use calamine::{open_workbook_auto, Reader};
use md5;
use rust_xlsxwriter::{Workbook, Format};
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter, QueryOrder, QuerySelect, PaginatorTrait, Set, TransactionTrait};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{info, error};
use tokio::task::JoinSet;

pub struct ExcelProcessor {
    db: sea_orm::DatabaseConnection,
}

impl ExcelProcessor {
    pub fn new(db: sea_orm::DatabaseConnection) -> Self {
        Self { db }
    }

    /// 生成文件哈希值
    async fn generate_file_hash(&self, file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
        let content = fs::read(file_path)?;
        let digest = md5::compute(content);
        Ok(format!("{:x}", digest))
    }

    /// 获取或创建文件元数据
    async fn get_or_create_file_metadata(&self, file_path: &str) -> Result<i32, sea_orm::DbErr> {
        // 获取文件信息
        let metadata = match fs::metadata(file_path) {
            Ok(meta) => meta,
            Err(e) => return Err(sea_orm::DbErr::Custom(format!("文件元数据读取失败: {}", e))),
        };

        let file_size = metadata.len() as i64;
        let file_hash = match self.generate_file_hash(file_path).await {
            Ok(hash) => hash,
            Err(e) => return Err(sea_orm::DbErr::Custom(format!("文件哈希生成失败: {}", e))),
        };

        let file_name = Path::new(file_path)
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        let now = chrono::Utc::now();

        // 尝试获取现有的文件元数据
        let existing_file: Option<files::Model> = files::Entity::find()
            .filter(files::Column::FilePath.eq(file_path))
            .one(&self.db)
            .await?;

        match existing_file {
            Some(file_model) => {
                // 更新现有文件的元数据
                let now = chrono::Utc::now();
                let updated_file = files::ActiveModel {
                    id: Set(file_model.id),
                    file_path: Set(file_model.file_path.clone()),
                    file_name: Set(file_model.file_name.clone()),
                    file_size: Set(file_size),
                    file_hash: Set(file_hash),
                    created_at: Set(file_model.created_at),
                    updated_at: Set(now),
                };
                updated_file.update(&self.db).await?;
                
                Ok(file_model.id)
            }
            None => {
                // 创建新的文件元数据
                let new_file = files::ActiveModel {
                    id: Default::default(),
                    file_path: Set(file_path.to_string()),
                    file_name: Set(file_name),
                    file_size: Set(file_size),
                    file_hash: Set(file_hash),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                let inserted = new_file.insert(&self.db).await?;
                Ok(inserted.id)
            }
        }
    }

    /// 检查文件是否需要更新（基于哈希值比较）
    async fn is_file_changed(&self, file_path: &str) -> Result<bool, Box<dyn std::error::Error>> {
        // 计算当前文件的哈希值
        let current_hash = self.generate_file_hash(file_path).await?;
        
        // 查询数据库中的文件记录
        let existing_file: Option<files::Model> = files::Entity::find()
            .filter(files::Column::FilePath.eq(file_path))
            .one(&self.db)
            .await?;

        match existing_file {
            Some(file_model) => {
                // 比较哈希值
                Ok(file_model.file_hash != current_hash)
            }
            None => {
                // 文件不存在于数据库中，需要导入
                Ok(true)
            }
        }
    }

    /// 删除指定文件的数据
    async fn delete_file_data(&self, file_id: i32) -> Result<(), sea_orm::DbErr> {
        // 删除关联的Excel数据
        excel_data::Entity::delete_many()
            .filter(excel_data::Column::FileId.eq(file_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    /// 读取Excel文件内容
    async fn read_excel_file(
        &self,
        file_path: &str,
    ) -> Result<Vec<HashMap<String, Value>>, Box<dyn std::error::Error>> {
        let mut workbook = open_workbook_auto(file_path)?;
        let sheet_names = workbook.sheet_names().to_owned();

        if sheet_names.is_empty() {
            return Ok(vec![]);
        }

        let sheet_name = &sheet_names[0];
        let range = workbook
            .worksheet_range(sheet_name)
            .map_err(|_| "无法读取工作表".to_string())?;

        let mut rows_data = Vec::new();

        if range.rows().count() == 0 {
            return Ok(vec![]);
        }

        // 获取列标题
        let headers: Vec<String> = range
            .rows()
            .next()
            .unwrap()
            .iter()
            .map(|cell| {
                match cell.to_string() {
                    s if s.is_empty() => "EMPTY".to_string(),
                    s => s,
                }
            })
            .collect();

        // 处理数据行
        for (_row_idx, row) in range.rows().enumerate().skip(1) {
            let mut row_data = HashMap::new();

            for (col_idx, cell) in row.iter().enumerate() {
                if col_idx < headers.len() {
                    let cell_str = cell.to_string();
                    let value = if cell_str.is_empty() {
                        Value::Null
                    } else {
                        // 检查是否为纯数字字符串
                        if cell_str.chars().all(|c| c.is_ascii_digit() || c == '.' || c == '-' || c == '+') {
                            // 如果数字长度超过15位，或者包含前导零，保持为字符串
                            // 这样可以避免身份证号码、电话号码等被错误转换
                            if cell_str.len() > 15 || (cell_str.len() > 1 && cell_str.starts_with('0') && !cell_str.contains('.')) {
                                Value::String(cell_str)
                            } else if let Ok(num) = cell_str.parse::<f64>() {
                                // 检查转换后的数字是否与原字符串一致（避免精度丢失）
                                let num_str = num.to_string();
                                if num_str == cell_str || (num.fract() == 0.0 && num.to_string().replace(".0", "") == cell_str) {
                                    Value::Number(serde_json::Number::from_f64(num).unwrap_or_else(|| serde_json::Number::from(0)))
                                } else {
                                    // 如果转换后不一致，说明有精度丢失，保持为字符串
                                    Value::String(cell_str)
                                }
                            } else {
                                Value::String(cell_str)
                            }
                        } else {
                            Value::String(cell_str)
                        }
                    };
                    row_data.insert(headers[col_idx].clone(), value);
                }
            }

            if !row_data.is_empty() {
                rows_data.push(row_data);
            }
        }

        Ok(rows_data)
    }

    /// 插入Excel数据到数据库
    async fn insert_excel_data(&self, file_id: i32, rows_data: Vec<HashMap<String, Value>>) -> Result<bool, sea_orm::DbErr> {
        if rows_data.is_empty() {
            return Ok(true);
        }

        let now = chrono::Utc::now();
        let mut records = Vec::new();

        for (index, row_data) in rows_data.iter().enumerate() {
            // 构建搜索文本
            let search_parts: Vec<String> = row_data
                .values()
                .map(|v| {
                    match v {
                        Value::String(s) => s.clone(),
                        Value::Number(n) => {
                            // 对于数字，检查是否为整数且位数较多
                            if let Some(f) = n.as_f64() {
                                if f.fract() == 0.0 && f.abs() >= 1e15 {
                                    // 对于大整数，使用整数格式避免科学计数法
                                    format!("{:.0}", f)
                                } else {
                                    n.to_string()
                                }
                            } else {
                                n.to_string()
                            }
                        },
                        Value::Bool(b) => b.to_string(),
                        _ => String::new(),
                    }
                })
                .collect();
            let search_text = search_parts.join(" ");

            let record = excel_data::ActiveModel {
                id: Default::default(),
                file_id: Set(file_id),
                import_time: Set(now),
                row_number: Set((index + 1) as i32),
                data_json: Set(serde_json::to_value(row_data.clone()).unwrap_or_default()),
                search_text: Set(search_text),
            };

            records.push(record);
        }

        // 批量插入数据
        if !records.is_empty() {
            // 使用事务确保数据一致性
            let txn = self.db.begin().await?;

            for record in records {
                record.insert(&txn).await?;
            }

            txn.commit().await?;

            info!("成功导入文件ID {}，共 {} 条记录", file_id, rows_data.len());
            Ok(true)
        } else {
            info!("文件ID {} 没有数据", file_id);
            Ok(true)
        }
    }

    /// 批量导入Excel文件（支持增量更新和多线程）
    pub async fn batch_import_excel_files(&self, folder_path: &str) -> Result<ImportStats, Box<dyn std::error::Error>> {
        self.batch_import_excel_files_with_options(folder_path, false, 4).await
    }

    /// 批量导入Excel文件（带选项和多线程支持）
    pub async fn batch_import_excel_files_with_options(&self, folder_path: &str, force_reimport: bool, max_concurrent_files: usize) -> Result<ImportStats, Box<dyn std::error::Error>> {
        let mut stats = ImportStats {
            success: 0,
            failed: 0,
            total: 0,
            skipped: 0,
        };

        let path = Path::new(folder_path);
        if !path.exists() {
            return Err(format!("文件夹不存在: {}", folder_path).into());
        }

        if !path.is_dir() {
            return Err(format!("路径不是文件夹: {}", folder_path).into());
        }

        let entries = fs::read_dir(path)?;
        let mut files_to_process = Vec::new();
        let excel_extensions = ["xlsx", "xls"];
        
        let mut all_files_count = 0;
        let mut processed_files_count = 0;
        let mut excel_files_count = 0;

        // 扫描Excel文件
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                all_files_count += 1;
                
                if path.is_file() {
                    processed_files_count += 1;
                    
                    let file_name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or("未知文件名".to_string());
                    
                    if let Some(extension) = path.extension() {
                        let ext_str = extension.to_string_lossy().to_lowercase();
                        info!("文件: {}, 扩展名: {}", file_name, ext_str);
                        
                        if excel_extensions.contains(&ext_str.as_str()) {
                            let file_path = path.to_string_lossy().to_string();
                            
                            // 检查文件是否需要更新（除非强制重新导入）
                            let needs_update = if force_reimport {
                                true
                            } else {
                                match self.is_file_changed(&file_path).await {
                                    Ok(changed) => changed,
                                    Err(e) => {
                                        error!("检查文件变化失败 {}: {}", file_path, e);
                                        true // 出错时默认需要更新
                                    }
                                }
                            };

                            if needs_update {
                                files_to_process.push(file_path);
                                stats.total += 1;
                                excel_files_count += 1;
                                info!("找到需要处理的Excel文件: {}", file_name);
                            } else {
                                stats.skipped += 1;
                                info!("跳过未变化的Excel文件: {}", file_name);
                            }
                        } else {
                            info!("跳过非Excel文件: {}, 扩展名: {}", file_name, ext_str);
                        }
                    } else {
                        info!("跳过无扩展名文件: {}", file_name);
                    }
                } else {
                    info!("跳过目录: {}", path.display());
                }
            }
        }
        
        info!("扫描完成 - 总文件数: {}, 处理文件数: {}, Excel文件数: {}, 需要更新: {}, 跳过: {}", 
              all_files_count, processed_files_count, excel_files_count, files_to_process.len(), stats.skipped);

        if files_to_process.is_empty() {
            info!("没有需要处理的文件");
            return Ok(stats);
        }

        // 使用多线程并行处理文件，并发数量可配置
        let chunk_size = std::cmp::max(1, max_concurrent_files); // 确保至少为1
        info!("使用并发处理，每批最多处理 {} 个文件", chunk_size);
        
        for chunk in files_to_process.chunks(chunk_size) {
            let mut tasks: JoinSet<Result<(), Box<dyn std::error::Error + Send + Sync>>> = JoinSet::new();
            
            for file_path in chunk {
                let file_path = file_path.clone();
                let db = self.db.clone();
                
                tasks.spawn(async move {
                    let processor = ExcelProcessor::new(db);
                    processor.process_single_file(&file_path, force_reimport).await
                });
            }
            
            // 等待当前批次的所有任务完成
            while let Some(result) = tasks.join_next().await {
                match result {
                    Ok(Ok(())) => {
                        stats.success += 1;
                    },
                    Ok(Err(e)) => {
                        error!("文件处理失败: {}", e);
                        stats.failed += 1;
                    },
                    Err(e) => {
                        error!("任务执行失败: {}", e);
                        stats.failed += 1;
                    }
                }
            }
        }

        info!("批量导入完成 - 成功: {}, 失败: {}, 总计: {}, 跳过: {}", stats.success, stats.failed, stats.total, stats.skipped);
        Ok(stats)
    }

    /// 处理单个文件（用于多线程调用）
    async fn process_single_file(&self, file_path: &str, force_reimport: bool) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("开始处理文件: {}", file_path);
        
        // 如果不是强制重新导入，检查文件是否需要更新
        if !force_reimport {
            match self.is_file_changed(file_path).await {
                Ok(false) => {
                    info!("文件未发生变化，跳过处理: {}", file_path);
                    return Ok(());
                },
                Ok(true) => {
                    info!("文件已发生变化，需要重新处理: {}", file_path);
                },
                Err(e) => {
                    error!("检查文件变化失败 {}: {}", file_path, e);
                    // 出错时默认需要更新
                }
            }
        }
        
        // 获取或创建文件元数据
        let file_id = match self.get_or_create_file_metadata(file_path).await {
            Ok(id) => {
                info!("文件元数据处理成功，文件ID: {}", id);
                id
            },
            Err(e) => {
                return Err(format!("处理文件元数据失败 {}: {}", file_path, e).into());
            }
        };

        // 删除现有数据
        if let Err(e) = self.delete_file_data(file_id).await {
            return Err(format!("删除现有数据失败 {}: {}", file_path, e).into());
        }
        
        info!("已删除文件 {} 的现有数据", file_path);

        // 读取Excel文件
        let rows_data = match self.read_excel_file(file_path).await {
            Ok(data) => {
                info!("文件读取成功 {}: 共 {} 行数据", file_path, data.len());
                data
            },
            Err(e) => {
                return Err(format!("文件读取失败 {}: {}", file_path, e).into());
            }
        };

        // 插入数据
        match self.insert_excel_data(file_id, rows_data).await {
            Ok(_) => {
                info!("文件数据导入成功: {}", file_path);
                Ok(())
            },
            Err(e) => {
                Err(format!("文件数据导入失败 {}: {}", file_path, e).into())
            }
        }
    }

    /// 搜索数据
    pub async fn search_data(&self, query_text: &str, limit: u64, offset: u64) -> Result<SearchResponse, sea_orm::DbErr> {
        use sea_orm::Condition;
        
        // 解析多关键词
        let keywords: Vec<&str> = query_text.trim().split_whitespace().filter(|k| !k.is_empty()).collect();
        
        if keywords.is_empty() {
            return Ok(SearchResponse {
                results: vec![],
                total: 0,
                limit: limit as i64,
                offset: offset as i64,
            });
        }

        // 构建搜索条件：每个关键词都必须在search_text中存在
        let mut condition = Condition::all();
        for keyword in &keywords {
            condition = condition.add(excel_data::Column::SearchText.contains(*keyword));
        }

        // 计算总数
        let total = excel_data::Entity::find()
            .filter(condition.clone())
            .count(&self.db)
            .await?;

        // 获取结果，包含文件信息
        let results: Vec<(excel_data::Model, Option<files::Model>)> = excel_data::Entity::find()
            .find_also_related(files::Entity)
            .filter(condition)
            .order_by_desc(excel_data::Column::ImportTime)
            .limit(limit)
            .offset(offset)
            .all(&self.db)
            .await?;

        // 转换为兼容的ExcelData结构
        let converted_results: Vec<ExcelData> = results
            .into_iter()
            .map(|(excel_model, file_model)| ExcelData {
                id: Some(excel_model.id),
                file_id: excel_model.file_id,
                import_time: excel_model.import_time,
                row_number: excel_model.row_number,
                data_json: excel_model.data_json.to_string(),
                search_text: excel_model.search_text,
                file_name: file_model.map(|f| f.file_name),
            })
            .collect();

        Ok(SearchResponse {
            results: converted_results,
            total: total as i64,
            limit: limit as i64,
            offset: offset as i64,
        })
    }

    /// 获取统计信息
    pub async fn get_statistics(&self) -> Result<StatsResponse, sea_orm::DbErr> {
        // 获取总记录数
        let total_records = excel_data::Entity::find()
            .count(&self.db)
            .await?;

        // 获取文件数
        let total_files = files::Entity::find()
            .count(&self.db)
            .await?;

        // 获取最新导入时间
        let latest_import_time = excel_data::Entity::find()
            .order_by_desc(excel_data::Column::ImportTime)
            .one(&self.db)
            .await?
            .map(|model| model.import_time)
            .unwrap_or_else(|| chrono::Utc::now());

        Ok(StatsResponse {
            total_rows: total_records as i64,
            total_files: total_files as i64,
            last_update: latest_import_time,
        })
    }

    /// 导出搜索结果到Excel文件
    pub async fn export_search_results(&self, query_text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        use sea_orm::Condition;
        
        // 解析多关键词
        let keywords: Vec<&str> = query_text.trim().split_whitespace().filter(|k| !k.is_empty()).collect();
        
        if keywords.is_empty() {
            return Err("搜索关键词不能为空".into());
        }

        // 构建搜索条件
        let mut condition = Condition::all();
        for keyword in &keywords {
            condition = condition.add(excel_data::Column::SearchText.contains(*keyword));
        }

        // 获取所有匹配的数据，包含文件信息
        let results: Vec<(excel_data::Model, Option<files::Model>)> = excel_data::Entity::find()
            .find_also_related(files::Entity)
            .filter(condition)
            .order_by_desc(excel_data::Column::ImportTime)
            .all(&self.db)
            .await?;

        if results.is_empty() {
            return Err("没有找到匹配的数据".into());
        }

        // 按文件分组数据
        let mut grouped_data: HashMap<String, Vec<(excel_data::Model, files::Model)>> = HashMap::new();
        
        for (excel_model, file_model_opt) in results {
            if let Some(file_model) = file_model_opt {
                let file_name = file_model.file_name.clone();
                grouped_data.entry(file_name).or_insert_with(Vec::new).push((excel_model, file_model));
            }
        }

        // 创建Excel工作簿
        let mut workbook = Workbook::new();
        
        // 创建格式
        let header_format = Format::new()
            .set_bold()
            .set_background_color("#4472C4")
            .set_font_color("#FFFFFF")
            .set_border(rust_xlsxwriter::FormatBorder::Thin);
            
        let data_format = Format::new()
            .set_border(rust_xlsxwriter::FormatBorder::Thin);

        // 为每个文件创建工作表
        for (file_name, file_data) in grouped_data.iter() {
            // 清理工作表名称（Excel工作表名称有限制）
            let sheet_name = self.sanitize_sheet_name(file_name);
            let worksheet = workbook.add_worksheet().set_name(&sheet_name)?;

            if file_data.is_empty() {
                continue;
            }

            // 获取所有唯一的列名
            let mut all_columns = std::collections::BTreeSet::new();
            all_columns.insert("行号".to_string());
            all_columns.insert("导入时间".to_string());
            
            for (excel_model, _) in file_data {
                if let Ok(data_obj) = serde_json::from_value::<HashMap<String, Value>>(excel_model.data_json.clone()) {
                    for key in data_obj.keys() {
                        all_columns.insert(key.clone());
                    }
                }
            }

            let columns: Vec<String> = all_columns.into_iter().collect();

            // 写入表头
            for (col_idx, column_name) in columns.iter().enumerate() {
                worksheet.write_string_with_format(0, col_idx as u16, column_name, &header_format)?;
            }

            // 写入数据
            for (row_idx, (excel_model, _)) in file_data.iter().enumerate() {
                let row = (row_idx + 1) as u32;
                
                for (col_idx, column_name) in columns.iter().enumerate() {
                    let col = col_idx as u16;
                    
                    let cell_value = match column_name.as_str() {
                        "行号" => excel_model.row_number.to_string(),
                        "导入时间" => excel_model.import_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                        _ => {
                            if let Ok(data_obj) = serde_json::from_value::<HashMap<String, Value>>(excel_model.data_json.clone()) {
                                match data_obj.get(column_name) {
                                    Some(Value::String(s)) => s.clone(),
                                    Some(Value::Number(n)) => n.to_string(),
                                    Some(Value::Bool(b)) => b.to_string(),
                                    Some(Value::Null) => String::new(),
                                    Some(v) => v.to_string(),
                                    None => String::new(),
                                }
                            } else {
                                String::new()
                            }
                        }
                    };
                    
                    worksheet.write_string_with_format(row, col, &cell_value, &data_format)?;
                }
            }

            // 自动调整列宽
            for col_idx in 0..columns.len() {
                worksheet.set_column_width(col_idx as u16, 15.0)?;
            }
        }

        // 保存到内存缓冲区
        let buffer = workbook.save_to_buffer()?;
        Ok(buffer)
    }

    /// 清理工作表名称，确保符合Excel规范
    fn sanitize_sheet_name(&self, name: &str) -> String {
        // Excel工作表名称限制：
        // - 最大31个字符
        // - 不能包含: \ / ? * [ ] :
        let mut sanitized = name
            .chars()
            .filter(|c| !['\\', '/', '?', '*', '[', ']', ':'].contains(c))
            .collect::<String>();
            
        if sanitized.len() > 31 {
            sanitized.truncate(28);
            sanitized.push_str("...");
        }
        
        if sanitized.is_empty() {
            sanitized = "Sheet1".to_string();
        }
        
        sanitized
    }
}