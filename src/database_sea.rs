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
    
    // 创建users表
    let create_users_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id SERIAL PRIMARY KEY,
            username TEXT UNIQUE NOT NULL,
            password_hash TEXT NOT NULL,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )
        "#.to_string()
    );

    db.execute(create_users_table).await?;
    info!("users表检查完成");

    // 创建workspaces表
    let create_workspaces_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS workspaces (
            id SERIAL PRIMARY KEY,
            owner_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            name TEXT NOT NULL,
            description TEXT,
            is_public BOOLEAN NOT NULL DEFAULT FALSE,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
            updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )
        "#.to_string()
    );

    db.execute(create_workspaces_table).await?;
    info!("workspaces表检查完成");

    // 创建auth_tokens表
    let create_auth_tokens_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS auth_tokens (
            id SERIAL PRIMARY KEY,
            user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
            token TEXT UNIQUE NOT NULL,
            expires_at TIMESTAMP WITH TIME ZONE NOT NULL,
            created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
        )
        "#.to_string()
    );

    db.execute(create_auth_tokens_table).await?;
    info!("auth_tokens表检查完成");

    // 创建files表
    let create_files_table = Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        r#"
        CREATE TABLE IF NOT EXISTS files (
            id SERIAL PRIMARY KEY,
            workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
            uploaded_by INTEGER REFERENCES users(id) ON DELETE SET NULL,
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
            workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE,
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

    // 增量升级旧表结构
    let schema_upgrades = vec![
        "ALTER TABLE files ADD COLUMN IF NOT EXISTS workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE",
        "ALTER TABLE files ADD COLUMN IF NOT EXISTS uploaded_by INTEGER REFERENCES users(id) ON DELETE SET NULL",
        "ALTER TABLE excel_data ADD COLUMN IF NOT EXISTS workspace_id INTEGER REFERENCES workspaces(id) ON DELETE CASCADE",
        "ALTER TABLE workspaces ADD COLUMN IF NOT EXISTS description TEXT",
        "ALTER TABLE workspaces ADD COLUMN IF NOT EXISTS is_public BOOLEAN NOT NULL DEFAULT FALSE",
    ];

    for sql in schema_upgrades {
        let statement = Statement::from_string(sea_orm::DatabaseBackend::Postgres, sql.to_string());
        db.execute(statement).await?;
    }
    info!("数据库增量升级检查完成");
    
    // 创建索引
    let indexes = vec![
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_users_username ON users(username)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_auth_tokens_token ON auth_tokens(token)",
        "CREATE INDEX IF NOT EXISTS idx_auth_tokens_user_id ON auth_tokens(user_id)",
        "CREATE INDEX IF NOT EXISTS idx_auth_tokens_expires_at ON auth_tokens(expires_at)",
        "CREATE INDEX IF NOT EXISTS idx_workspaces_owner_id ON workspaces(owner_id)",
        "CREATE INDEX IF NOT EXISTS idx_workspaces_is_public ON workspaces(is_public)",
        "CREATE UNIQUE INDEX IF NOT EXISTS idx_workspaces_owner_name_unique ON workspaces(owner_id, name)",
        "CREATE INDEX IF NOT EXISTS idx_files_workspace_id ON files(workspace_id)",
        "CREATE INDEX IF NOT EXISTS idx_files_uploaded_by ON files(uploaded_by)",
        "CREATE INDEX IF NOT EXISTS idx_excel_data_workspace_id ON excel_data(workspace_id)",
        "CREATE INDEX IF NOT EXISTS idx_excel_data_workspace_import_time ON excel_data(workspace_id, import_time DESC)",
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
