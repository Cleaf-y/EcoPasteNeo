//! Explicit, isolated management fixtures; never reset or seed an existing history.
use anyhow::{Result, bail};
use ecopaste_neo_app::options::Options;
use ecopaste_neo_core::{
    db,
    paths::{Environment, Storage},
};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if !arguments.iter().any(|argument| argument == "--data-parent") {
        bail!("An explicit temporary --data-parent is required");
    }
    let options = Options::parse(arguments)?;
    if options.environment != Environment::Dev {
        bail!("Management fixtures require dev storage");
    }
    let paths = Storage::new(&options.data_parent, options.environment)?.open()?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let pool = db::open(&paths).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clipboard_items").fetch_one(&pool).await?;
        if count != 0 {
            bail!("Database already contains {count} records; refusing to reset it");
        }
        let directory = paths.state_dir().join("menu08");
        std::fs::create_dir_all(&directory)?;
        let file = directory.join("keep-original.txt");
        std::fs::write(&file, "MENU08-ORIGINAL-FILE-MUST-STAY\n")?;
        let name = format!("{}.png", "cd".repeat(32));
        let image = paths.resources_dir().join("clipboard-images/origin/cd").join(&name);
        std::fs::create_dir_all(image.parent().unwrap())?;
        image::RgbImage::from_pixel(640, 360, image::Rgb([60, 150, 220])).save(&image)?;
        let fixtures = [
            ("menu08-image", "image", name.as_str(), Some("menu08-image")),
            ("menu08-files", "files", file.to_str().unwrap(), Some("menu08-files")),
            ("menu08-delete", "text", "menu08-delete · 专用删除记录", None),
            ("menu08-B", "text", "menu08-pair B · MENU08-PASTE-B", None),
            ("menu08-A", "text", "menu08-pair A · MENU08-PASTE-A", None),
        ];
        let mut transaction = pool.begin().await?;
        for (index, (id, kind, content, note)) in fixtures.into_iter().enumerate() {
            sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,search_text,summary,note,file_types,width,height,size,platform,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,'macos',datetime('2026-11-18','+' || ? || ' seconds'),datetime('2026-11-18','+' || ? || ' seconds'))")
                .bind(id).bind(kind).bind(content).bind(id).bind((kind == "text").then_some(content)).bind((kind == "text").then_some(content)).bind(note)
                .bind((kind == "files").then_some("f")).bind((kind == "image").then_some(640)).bind((kind == "image").then_some(360))
                .bind(if kind == "image" { std::fs::metadata(&image)?.len() as i64 } else { content.len() as i64 }).bind(index as i64).bind(index as i64)
                .execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        pool.close().await;
        println!("Created 5 isolated management records.\nDatabase: {}\nOriginal file to preserve: {}", paths.db_path().display(), file.display());
        Ok(())
    })
}
