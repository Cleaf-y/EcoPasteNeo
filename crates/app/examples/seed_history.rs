//! Explicitly invoked fixture builder. Never runs as part of application startup.

use anyhow::{Context, Result, bail};
use ecopaste_neo_app::options::Options;
use ecopaste_neo_core::{
    db,
    paths::{Environment, Storage},
};
use image::{ImageBuffer, Rgba};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if !arguments.iter().any(|arg| arg == "--data-parent") {
        bail!("Fixture creation requires an explicit --data-parent temporary directory");
    }
    let options = Options::parse(arguments)?;
    if options.environment != Environment::Dev {
        bail!("Fixtures require --environment dev");
    }
    let paths = Storage::new(&options.data_parent, options.environment)?.open()?;
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let pool = db::open(&paths).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clipboard_items").fetch_one(&pool).await?;
        if count != 0 { bail!("Fixture database already contains {count} records; refusing to reset it"); }
        let fixture_files = paths.state_dir().join("fixture-files");
        std::fs::create_dir_all(&fixture_files)?;
        let sample = fixture_files.join("sample.txt");
        std::fs::write(&sample, "EcoPasteNeo isolated history fixture\n")?;
        let mut transaction = pool.begin().await?;
        for index in 0..1000 {
            let keyword = if index % 2 == 0 { "alpha" } else { "beta" };
            let text = format!("needle{index:04} · {keyword} · 中文搜索 · 记录 {index:04}\n{}", "高低不同的真实 SQLite 历史卡片，支持搜索和分页。\n".repeat(index % 7));
            let (kind, content, summary, file_types, width, height) = match index % 10 {
                0 | 1 => {
                    let width = if index % 2 == 0 { 640 } else { 240 };
                    let height = if index % 2 == 0 { 240 } else { 480 };
                    let file_name = format!("{index:064x}.png");
                    let path = paths.resources_dir().join("clipboard-images/origin").join(&file_name[..2]).join(&file_name);
                    std::fs::create_dir_all(path.parent().context("Image parent")?)?;
                    ImageBuffer::from_fn(width, height, |x, y| Rgba([((x + index as u32) % 256) as u8, ((y * 2) % 256) as u8, 140, 255])).save(&path)?;
                    ("image", file_name, None, None, Some(width as i64), Some(height as i64))
                }
                2 => {
                    let missing = fixture_files.join(format!("missing-{index:04}.pdf"));
                    ("files", format!("{}\n{}\n{}", sample.display(), fixture_files.display(), missing.display()), None, Some("f,d,f"), None, None)
                }
                _ => ("text", text.clone(), Some(text.chars().take(512).collect::<String>()), None, None, None),
            };
            sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,search_text,summary,file_types,width,height,size,is_pinned,platform,note,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,datetime('2026-11-18','+' || ? || ' seconds'),datetime('2026-11-18','+' || ? || ' seconds'))")
                .bind(format!("fixture-{index:04}")).bind(kind).bind(&content).bind(format!("fixture-{index:04}"))
                .bind(&text).bind(summary).bind(file_types).bind(width).bind(height).bind(content.len() as i64)
                .bind(index == 997).bind("macos").bind(format!("{keyword} · needle{index:04} · 中文搜索"))
                .bind(index as i64).bind(index as i64).execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        pool.close().await;
        println!("Created 1000 records (700 text, 200 images, 100 file groups).\nDatabase: {}", paths.db_path().display());
        Ok(())
    })
}
