use crate::models::{ExcelData, SearchResponse, StatsResponse};
use crate::models::entity::{excel_data, files, workspaces};
use calamine::{open_workbook_auto, Reader};
use md5;
use rust_xlsxwriter::{Workbook, Format};
use sea_orm::{ActiveModelTrait, ColumnTrait, Condition, EntityTrait, QueryFilter, QueryOrder, QuerySelect, PaginatorTrait, Set, TransactionTrait};
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::{info, error};

pub struct ExcelProcessor {
    db: sea_orm::DatabaseConnection,
}

impl ExcelProcessor {
    pub fn new(db: sea_orm::DatabaseConnection) -> Self {
        Self { db }
    }

    /// 检测行数据中可疑的Unicode转义序列
    fn find_suspicious_escapes(row: &HashMap<String, Value>) -> Vec<(String, String)> {
        let mut suspicious_fields = Vec::new();
        for (key, value) in row {
            if let Value::String(s) = value {
                if s.contains("\\u") || s.contains("\u{005C}u") || s.contains('\u{0000}') {
                    suspicious_fields.push((key.clone(), s.clone()));
                }
            }
        }
        suspicious_fields
    }

    /// 清理行数据中的问题字符（包括字段名和字段值）
    fn clean_row_data(row: &mut HashMap<String, Value>) {
        // 首先清理字段名，需要重新构建HashMap
        let mut cleaned_row = HashMap::new();
        
        for (key, value) in row.drain() {
            // 清理字段名 - 只移除真正有问题的控制字符，保留Unicode字符
            let cleaned_key = key
                .replace('\u{0000}', "") // 移除空字符
                .replace('\u{FEFF}', "") // 移除BOM字符
                .replace('\u{200B}', "") // 移除零宽空格
                .replace('\u{200C}', "") // 移除零宽非连接符
                .replace('\u{200D}', "") // 移除零宽连接符
                .chars()
                .filter(|c| !c.is_control() || c.is_whitespace()) // 只过滤控制字符，保留空白字符和所有可见字符
                .collect::<String>()
                .trim()
                .to_string();
            
            // 清理字段值
            let cleaned_value = match value {
                Value::String(s) => {
                    let cleaned_str = s
                        .replace('\u{0000}', "") // 移除空字符
                        .replace('\u{FEFF}', "") // 移除BOM字符
                        .replace('\u{200B}', "") // 移除零宽空格
                        .replace('\u{200C}', "") // 移除零宽非连接符
                        .replace('\u{200D}', "") // 移除零宽连接符
                        .chars()
                        .filter(|c| !c.is_control() || c.is_whitespace()) // 只过滤控制字符，保留空白字符和所有可见字符
                        .collect::<String>()
                        .trim()
                        .to_string();
                    Value::String(cleaned_str)
                }
                other => other,
            };
            
            // 只保留有效的字段名
            if !cleaned_key.is_empty() {
                cleaned_row.insert(cleaned_key, cleaned_value);
            }
        }
        
        // 将清理后的数据放回原HashMap
        *row = cleaned_row;
    }

    /// 生成文件哈希值
    async fn generate_file_hash(&self, file_path: &str) -> Result<String, Box<dyn std::error::Error>> {
        let content = fs::read(file_path)?;
        let digest = md5::compute(content);
        Ok(format!("{:x}", digest))
    }

    /// 获取或创建文件元数据
    async fn get_or_create_file_metadata(
        &self,
        file_path: &str,
        workspace_id: Option<i32>,
        uploaded_by: Option<i32>,
    ) -> Result<i32, sea_orm::DbErr> {
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
        let mut file_filter = Condition::all().add(files::Column::FilePath.eq(file_path));
        file_filter = if let Some(wid) = workspace_id {
            file_filter.add(files::Column::WorkspaceId.eq(wid))
        } else {
            file_filter.add(files::Column::WorkspaceId.is_null())
        };

        let existing_file: Option<files::Model> = files::Entity::find()
            .filter(file_filter)
            .one(&self.db)
            .await?;

        match existing_file {
            Some(file_model) => {
                // 更新现有文件的元数据
                let now = chrono::Utc::now();
                let updated_file = files::ActiveModel {
                    id: Set(file_model.id),
                    workspace_id: Set(file_model.workspace_id),
                    uploaded_by: Set(uploaded_by.or(file_model.uploaded_by)),
                    file_path: Set(file_model.file_path.clone()),
                    file_name: Set(file_model.file_name.clone()),
                    file_size: Set(file_size),
                    file_hash: Set(file_hash),
                    field_order: Set(file_model.field_order),
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
                    workspace_id: Set(workspace_id),
                    uploaded_by: Set(uploaded_by),
                    file_path: Set(file_path.to_string()),
                    file_name: Set(file_name),
                    file_size: Set(file_size),
                    file_hash: Set(file_hash),
                    field_order: Set(None),
                    created_at: Set(now),
                    updated_at: Set(now),
                };
                let inserted = new_file.insert(&self.db).await?;
                Ok(inserted.id)
            }
        }
    }

    /// 检查文件是否需要更新（基于哈希值比较）
    async fn is_file_changed(
        &self,
        file_path: &str,
        workspace_id: Option<i32>,
    ) -> Result<bool, Box<dyn std::error::Error>> {
        // 计算当前文件的哈希值
        let current_hash = self.generate_file_hash(file_path).await?;
        
        // 查询数据库中的文件记录
        let mut file_filter = Condition::all().add(files::Column::FilePath.eq(file_path));
        file_filter = if let Some(wid) = workspace_id {
            file_filter.add(files::Column::WorkspaceId.eq(wid))
        } else {
            file_filter.add(files::Column::WorkspaceId.is_null())
        };

        let existing_file: Option<files::Model> = files::Entity::find()
            .filter(file_filter)
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
    ) -> Result<(Vec<(String, Vec<HashMap<String, Value>>)>, Vec<String>), Box<dyn std::error::Error>> {
        let mut workbook = open_workbook_auto(file_path)?;
        let sheet_names = workbook.sheet_names().to_owned();

        if sheet_names.is_empty() {
            return Ok((vec![], vec![]));
        }

        let mut all_sheets_data = Vec::new();
        let mut all_headers = Vec::new();

        // 遍历所有工作表
        for sheet_name in &sheet_names {
            let range = match workbook.worksheet_range(sheet_name) {
                Ok(range) => range,
                Err(_) => {
                    info!("跳过无法读取的工作表: {}", sheet_name);
                    continue;
                }
            };

            let mut rows_data = Vec::new();

            if range.rows().count() == 0 {
                info!("工作表 {} 为空，跳过", sheet_name);
                continue;
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

            if !rows_data.is_empty() {
                info!("工作表 {} 读取完成，共 {} 行数据", sheet_name, rows_data.len());
                all_sheets_data.push((sheet_name.clone(), rows_data));
                
                // 收集所有唯一的列标题
                for header in headers {
                    if !all_headers.contains(&header) {
                        all_headers.push(header);
                    }
                }
            } else {
                info!("工作表 {} 没有有效数据", sheet_name);
            }
        }

        Ok((all_sheets_data, all_headers))
    }

    /// 插入Excel数据到数据库
    async fn insert_excel_data(
        &self,
        workspace_id: Option<i32>,
        file_id: i32,
        file_path: &str,
        sheet_name: &str,
        rows_data: Vec<HashMap<String, Value>>,
    ) -> Result<bool, sea_orm::DbErr> {
        if rows_data.is_empty() {
            return Ok(true);
        }

        let now = chrono::Utc::now();
        let mut records = Vec::new();

        for (index, mut row_data) in rows_data.into_iter().enumerate() {
            // 清理数据中的问题字符
            Self::clean_row_data(&mut row_data);
            
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
                workspace_id: Set(workspace_id),
                file_id: Set(file_id),
                import_time: Set(now),
                row_number: Set((index + 1) as i32),
                data_json: Set(serde_json::to_value(row_data.clone()).unwrap_or_default()),
                search_text: Set(search_text),
                sheet_name: Set(sheet_name.to_string()),
            };

            records.push(((index + 1), row_data.clone(), record));
        }

        // 使用事务逐条插入，并在失败时打印详细上下文
        if !records.is_empty() {
            let txn = self.db.begin().await?;
            let total_records = records.len();

            for (row_no, row_data, record) in records {
                if let Err(e) = record.insert(&txn).await {
                    let suspicious_fields = Self::find_suspicious_escapes(&row_data);
                    let raw_json = serde_json::to_string(&row_data).unwrap_or_default();
                    error!(
                        "数据行导入失败: 文件={} 工作表={} 行号={} 错误={} 可疑字段={:?} 原始JSON={}",
                        file_path,
                        sheet_name,
                        row_no,
                        e,
                        suspicious_fields,
                        raw_json
                    );
                    // 出现错误直接返回，让上层日志保持"工作表 X 数据导入失败"
                    return Err(e);
                }
            }

            txn.commit().await?;

            info!("成功导入文件ID {}，工作表 {}，共 {} 条记录", file_id, sheet_name, total_records);
            Ok(true)
        } else {
            info!("文件ID {} 没有数据", file_id);
            Ok(true)
        }
    }

    /// 处理单个文件（用于多线程调用）
    async fn process_single_file(
        &self,
        file_path: &str,
        force_reimport: bool,
        workspace_id: Option<i32>,
        uploaded_by: Option<i32>,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        info!("开始处理文件: {}", file_path);
        
        // 如果不是强制重新导入，检查文件是否需要更新
        if !force_reimport {
            match self.is_file_changed(file_path, workspace_id).await {
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
        let file_id = match self.get_or_create_file_metadata(file_path, workspace_id, uploaded_by).await {
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
        let (all_sheets_data, field_order) = match self.read_excel_file(file_path).await {
            Ok((data, headers)) => {
                let total_rows: usize = data.iter().map(|(_, rows)| rows.len()).sum();
                info!("文件读取成功 {}: 共 {} 个工作表，{} 行数据", file_path, data.len(), total_rows);
                (data, headers)
            },
            Err(e) => {
                return Err(format!("文件读取失败 {}: {}", file_path, e).into());
            }
        };

        // 更新文件的字段顺序信息
        if let Err(e) = self.update_file_field_order(file_id, &field_order).await {
            error!("更新文件字段顺序失败 {}: {}", file_path, e);
        }

        // 插入每个工作表的数据
        for (sheet_name, rows_data) in all_sheets_data {
            match self.insert_excel_data(workspace_id, file_id, file_path, &sheet_name, rows_data).await {
                Ok(_) => {
                    info!("工作表 {} 数据导入成功", sheet_name);
                },
                Err(e) => {
                    error!("工作表 {} 数据导入失败: {}", sheet_name, e);
                    return Err(format!("工作表 {} 数据导入失败: {}", sheet_name, e).into());
                }
            }
        }

        info!("文件数据导入成功: {}", file_path);
        Ok(())
    }

    /// 更新文件的字段顺序信息
    async fn update_file_field_order(&self, file_id: i32, field_order: &[String]) -> Result<(), sea_orm::DbErr> {
        use sea_orm::{ActiveModelTrait, EntityTrait, Set};
        
        // 将字段顺序转换为JSON
        let field_order_json = serde_json::to_value(field_order)
            .map_err(|e| sea_orm::DbErr::Custom(format!("序列化字段顺序失败: {}", e)))?;
        
        // 查找文件记录
        let file_model = files::Entity::find_by_id(file_id)
            .one(&self.db)
            .await?;
        
        if let Some(file) = file_model {
            let mut file_active: files::ActiveModel = file.into();
            file_active.field_order = Set(Some(field_order_json));
            file_active.updated_at = Set(chrono::Utc::now());
            file_active.update(&self.db).await?;
        }
        
        Ok(())
    }

    pub async fn import_uploaded_file(
        &self,
        workspace_id: i32,
        file_path: &str,
        uploaded_by: i32,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.process_single_file(file_path, true, Some(workspace_id), Some(uploaded_by)).await
    }

    async fn get_public_workspace_ids(&self) -> Result<Vec<i32>, sea_orm::DbErr> {
        workspaces::Entity::find()
            .select_only()
            .column(workspaces::Column::Id)
            .filter(workspaces::Column::IsPublic.eq(true))
            .into_tuple::<i32>()
            .all(&self.db)
            .await
    }

    async fn get_scored_results(
        &self,
        query_text: &str,
        workspace_id: Option<i32>,
        only_public_workspaces: bool,
    ) -> Result<Vec<((excel_data::Model, Option<files::Model>), i32)>, sea_orm::DbErr> {
        let keywords: Vec<&str> = query_text
            .trim()
            .split_whitespace()
            .filter(|k| !k.is_empty())
            .collect();

        if keywords.is_empty() {
            return Ok(vec![]);
        }

        let mut condition = Condition::any();
        for keyword in &keywords {
            condition = condition.add(excel_data::Column::SearchText.contains(*keyword));
        }

        if let Some(wid) = workspace_id {
            condition = Condition::all()
                .add(condition)
                .add(excel_data::Column::WorkspaceId.eq(wid));
        } else if only_public_workspaces {
            let public_workspace_ids = self.get_public_workspace_ids().await?;
            if public_workspace_ids.is_empty() {
                return Ok(vec![]);
            }
            condition = Condition::all()
                .add(condition)
                .add(excel_data::Column::WorkspaceId.is_in(public_workspace_ids));
        }

        let all_results: Vec<(excel_data::Model, Option<files::Model>)> = excel_data::Entity::find()
            .find_also_related(files::Entity)
            .filter(condition)
            .all(&self.db)
            .await?;

        let mut scored_results: Vec<((excel_data::Model, Option<files::Model>), i32)> = all_results
            .into_iter()
            .map(|result| {
                let search_text = &result.0.search_text;
                let mut score = 0;
                for keyword in &keywords {
                    if search_text.contains(*keyword) {
                        score += 1;
                    }
                }
                if search_text.contains(query_text) {
                    score += keywords.len() as i32;
                }
                (result, score)
            })
            .collect();

        scored_results.sort_by(|a, b| match b.1.cmp(&a.1) {
            std::cmp::Ordering::Equal => b.0.0.import_time.cmp(&a.0.0.import_time),
            other => other,
        });

        Ok(scored_results)
    }

    async fn search_with_scope(
        &self,
        query_text: &str,
        limit: u64,
        offset: u64,
        workspace_id: Option<i32>,
        only_public_workspaces: bool,
    ) -> Result<SearchResponse, sea_orm::DbErr> {
        let scored_results = self
            .get_scored_results(query_text, workspace_id, only_public_workspaces)
            .await?;
        let total = scored_results.len() as i64;

        let paginated_results: Vec<(excel_data::Model, Option<files::Model>)> = scored_results
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize)
            .map(|(result, _score)| result)
            .collect();

        let converted_results: Vec<ExcelData> = paginated_results
            .into_iter()
            .map(|(excel_model, file_model)| ExcelData {
                id: Some(excel_model.id),
                workspace_id: excel_model.workspace_id,
                file_id: excel_model.file_id,
                import_time: excel_model.import_time,
                row_number: excel_model.row_number,
                data_json: excel_model.data_json.to_string(),
                search_text: excel_model.search_text,
                sheet_name: excel_model.sheet_name,
                file_name: file_model.as_ref().map(|f| f.file_name.clone()),
                field_order: file_model.and_then(|f| f.field_order),
            })
            .collect();

        Ok(SearchResponse {
            results: converted_results,
            total,
            limit: limit as i64,
            offset: offset as i64,
        })
    }

    pub async fn search_workspace_data(
        &self,
        workspace_id: i32,
        query_text: &str,
        limit: u64,
        offset: u64,
    ) -> Result<SearchResponse, sea_orm::DbErr> {
        self.search_with_scope(query_text, limit, offset, Some(workspace_id), false)
            .await
    }

    pub async fn search_public_data(
        &self,
        query_text: &str,
        limit: u64,
        offset: u64,
    ) -> Result<SearchResponse, sea_orm::DbErr> {
        self.search_with_scope(query_text, limit, offset, None, true).await
    }

    pub async fn get_workspace_statistics(&self, workspace_id: i32) -> Result<StatsResponse, sea_orm::DbErr> {
        let total_records = excel_data::Entity::find()
            .filter(excel_data::Column::WorkspaceId.eq(workspace_id))
            .count(&self.db)
            .await?;

        let total_files = files::Entity::find()
            .filter(files::Column::WorkspaceId.eq(workspace_id))
            .count(&self.db)
            .await?;

        let latest_import_time = excel_data::Entity::find()
            .filter(excel_data::Column::WorkspaceId.eq(workspace_id))
            .order_by_desc(excel_data::Column::ImportTime)
            .one(&self.db)
            .await?
            .map(|model| model.import_time)
            .unwrap_or_else(chrono::Utc::now);

        Ok(StatsResponse {
            total_rows: total_records as i64,
            total_files: total_files as i64,
            last_update: latest_import_time,
        })
    }

    pub async fn get_public_statistics(&self) -> Result<StatsResponse, sea_orm::DbErr> {
        let public_workspace_ids = self.get_public_workspace_ids().await?;
        if public_workspace_ids.is_empty() {
            return Ok(StatsResponse {
                total_rows: 0,
                total_files: 0,
                last_update: chrono::Utc::now(),
            });
        }

        let total_records = excel_data::Entity::find()
            .filter(excel_data::Column::WorkspaceId.is_in(public_workspace_ids.clone()))
            .count(&self.db)
            .await?;

        let total_files = files::Entity::find()
            .filter(files::Column::WorkspaceId.is_in(public_workspace_ids.clone()))
            .count(&self.db)
            .await?;

        let latest_import_time = excel_data::Entity::find()
            .filter(excel_data::Column::WorkspaceId.is_in(public_workspace_ids))
            .order_by_desc(excel_data::Column::ImportTime)
            .one(&self.db)
            .await?
            .map(|model| model.import_time)
            .unwrap_or_else(chrono::Utc::now);

        Ok(StatsResponse {
            total_rows: total_records as i64,
            total_files: total_files as i64,
            last_update: latest_import_time,
        })
    }

    pub async fn export_workspace_search_results(
        &self,
        workspace_id: i32,
        query_text: &str,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.export_search_results_with_scope(query_text, Some(workspace_id), false).await
    }

    pub async fn export_public_search_results(&self, query_text: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        self.export_search_results_with_scope(query_text, None, true).await
    }

    async fn export_search_results_with_scope(
        &self,
        query_text: &str,
        workspace_id: Option<i32>,
        only_public_workspaces: bool,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let scored_results = self
            .get_scored_results(query_text, workspace_id, only_public_workspaces)
            .await?;

        if scored_results.is_empty() {
            return Err("没有找到匹配的数据".into());
        }

        let mut grouped_data: HashMap<String, Vec<(excel_data::Model, files::Model)>> = HashMap::new();

        for ((excel_model, file_model_opt), _score) in scored_results {
            if let Some(file_model) = file_model_opt {
                let file_name = file_model.file_name.clone();
                grouped_data.entry(file_name).or_insert_with(Vec::new).push((excel_model, file_model));
            }
        }

        let mut workbook = Workbook::new();
        let header_format = Format::new()
            .set_bold()
            .set_background_color("#4472C4")
            .set_font_color("#FFFFFF")
            .set_border(rust_xlsxwriter::FormatBorder::Thin);
        let data_format = Format::new()
            .set_border(rust_xlsxwriter::FormatBorder::Thin);

        for (file_name, file_data) in grouped_data.iter() {
            let sheet_name = self.sanitize_sheet_name(file_name);
            let worksheet = workbook.add_worksheet().set_name(&sheet_name)?;

            if file_data.is_empty() {
                continue;
            }

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

            for (col_idx, column_name) in columns.iter().enumerate() {
                worksheet.write_string_with_format(0, col_idx as u16, column_name, &header_format)?;
            }

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

            for col_idx in 0..columns.len() {
                worksheet.set_column_width(col_idx as u16, 15.0)?;
            }
        }

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
