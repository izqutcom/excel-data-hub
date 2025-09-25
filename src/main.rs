mod database_sea;
mod excel_processor_sea;
mod web_server;
mod models;
mod utils;
mod i18n_manager;

use database_sea::connect_database;
use excel_processor_sea::ExcelProcessor;
use std::env;
use tracing::{info, error, warn};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 初始化日志
    tracing_subscriber::fmt::init();

    // 加载.env文件
    dotenv::dotenv().ok();

    // 从环境变量获取配置
    let excel_folder = env::var("EXCEL_FOLDER_PATH")
        .unwrap_or_else(|_| "./excel_files".to_string());
    let port: u16 = env::var("PORT")
        .unwrap_or_else(|_| "8000".to_string())
        .parse()
        .expect("PORT必须是一个有效的数字");
    
    // 检查是否强制重新导入所有文件
    let force_reimport = env::var("FORCE_REIMPORT")
        .unwrap_or_else(|_| "false".to_string())
        .to_lowercase() == "true";

    // 获取并发处理文件数量配置
    let max_concurrent_files: usize = env::var("MAX_CONCURRENT_FILES")
        .unwrap_or_else(|_| "4".to_string())
        .parse()
        .unwrap_or_else(|_| {
            warn!("MAX_CONCURRENT_FILES配置无效，使用默认值4");
            4
        });

    info!("使用Excel文件夹路径: {}", excel_folder);
    info!("强制重新导入: {}", force_reimport);
    info!("最大并发文件数: {}", max_concurrent_files);

    // 连接数据库
    let db = connect_database().await?;

    // 创建ExcelProcessor实例
    let processor = ExcelProcessor::new(db.clone());

    // 处理Excel文件夹中的所有文件
    if let Err(e) = processor.batch_import_excel_files_with_options(&excel_folder, force_reimport, max_concurrent_files).await {
        error!("Excel文件处理失败: {}", e);
        // 不要因为Excel处理失败而退出程序，继续启动Web服务器
    } else {
        info!("所有Excel文件处理完成");
    }

    // 启动Web服务器
    info!("正在启动Web服务器，端口: {}", port);
    match web_server::start_server(db, port).await {
        Ok(_) => {
            info!("Web服务器启动成功！");
            Ok(())
        },
        Err(e) => {
            error!("Web服务器启动失败: {:?}", e);
            error!("错误详情: {}", e);
            // 打印错误的源链
            let mut source = e.source();
            while let Some(err) = source {
                error!("  由于: {}", err);
                source = err.source();
            }
            Err(e)
        }
    }
}