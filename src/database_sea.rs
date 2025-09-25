use sea_orm::{Database, DatabaseConnection, DbErr, ConnectionTrait};
use std::env;
use tracing::info;

pub async fn connect_database() -> Result<DatabaseConnection, DbErr> {
    // 从环境变量获取数据库URL，必须是PostgreSQL
    let database_url = env::var("DATABASE_URL")
        .expect("DATABASE_URL环境变量必须设置为PostgreSQL连接字符串");
    
    if !database_url.starts_with("postgres://") && !database_url.starts_with("postgresql://") {
        return Err(DbErr::Custom("DATABASE_URL必须是PostgreSQL连接字符串".to_string()));
    }
    
    info!("正在使用Sea-ORM连接PostgreSQL数据库: {}", database_url);
    
    // 使用Sea-ORM连接数据库
    let db = Database::connect(&database_url).await?;
    
    // 创建数据库表（如果不存在）
    create_tables_if_not_exists(&db).await?;
    
    Ok(db)
}

async fn create_tables_if_not_exists(db: &DatabaseConnection) -> Result<(), DbErr> {
    use sea_orm::Statement;
    
    info!("检查并创建数据库表...");
    
    // 创建files表
    let create_files_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id SERIAL PRIMARY KEY,
            file_path TEXT UNIQUE NOT NULL,
            file_name TEXT NOT NULL,
            file_size BIGINT NOT NULL,
            file_hash TEXT NOT NULL,
            field_order JSONB,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )
        "#.to_string()
    );
    
    db.execute(create_files_table).await?;
    info!("files表检查完成");
    
    // 创建excel_data表
    let create_excel_data_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS excel_data (
            id SERIAL PRIMARY KEY,
            file_id INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
            import_time TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
            row_number INTEGER NOT NULL,
            sheet_name TEXT NOT NULL DEFAULT 'Sheet1',
            data_json JSONB NOT NULL,
            search_text TEXT NOT NULL
        )
        "#.to_string()
    );
    
    db.execute(create_excel_data_table).await?;
    info!("excel_data表检查完成");
    
    // 创建索引
    let indexes = vec![
        "CREATE INDEX IF NOT EXISTS idx_excel_data_search_text ON excel_data(search_text)",
        "CREATE INDEX IF NOT EXISTS idx_excel_data_file_id ON excel_data(file_id)",
        "CREATE INDEX IF NOT EXISTS idx_excel_data_import_time ON excel_data(import_time)",
        "CREATE INDEX IF NOT EXISTS idx_excel_data_data_json ON excel_data USING GIN (data_json)",
        "CREATE INDEX IF NOT EXISTS idx_files_file_path ON files(file_path)",
        "CREATE INDEX IF NOT EXISTS idx_files_file_hash ON files(file_hash)",
    ];
    
    for index_sql in indexes {
        let statement = Statement::from_string(sea_orm::DatabaseBackend::Postgres, index_sql.to_string());
        db.execute(statement).await?;
    }
    
    info!("数据库索引检查完成");
    info!("数据库表和索引初始化完成");
    
    Ok(())
}