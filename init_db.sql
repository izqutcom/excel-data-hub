-- 创建files表
CREATE TABLE IF NOT EXISTS public.files (
    id SERIAL PRIMARY KEY,
    file_path TEXT UNIQUE NOT NULL,
    file_name TEXT NOT NULL,
    file_size BIGINT NOT NULL,
    file_hash TEXT NOT NULL,
    field_order JSONB,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT NOW(),
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT NOW()
);

-- 创建excel_data表
CREATE TABLE IF NOT EXISTS public.excel_data (
    id SERIAL PRIMARY KEY,
    file_id INTEGER NOT NULL REFERENCES public.files(id) ON DELETE CASCADE,
    import_time TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    row_number INTEGER NOT NULL,
    sheet_name TEXT NOT NULL DEFAULT 'Sheet1',
    data_json JSONB NOT NULL,
    search_text TEXT NOT NULL
);

-- 创建索引
CREATE INDEX IF NOT EXISTS idx_excel_data_search_text ON public.excel_data(search_text);
CREATE INDEX IF NOT EXISTS idx_excel_data_file_id ON public.excel_data(file_id);
CREATE INDEX IF NOT EXISTS idx_excel_data_import_time ON public.excel_data(import_time);
CREATE INDEX IF NOT EXISTS idx_excel_data_data_json ON public.excel_data USING GIN (data_json);
CREATE INDEX IF NOT EXISTS idx_files_file_path ON public.files(file_path);
CREATE INDEX IF NOT EXISTS idx_files_file_hash ON public.files(file_hash);