//! Explicit file-drag fixtures and empty Finder destinations, isolated from real history.
use anyhow::{Result, bail};
use ecopaste_neo_app::options::Options;
use ecopaste_neo_core::{
    db,
    paths::{Environment, Storage},
};
use image::{ImageBuffer, Rgba};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if !arguments.iter().any(|argument| argument == "--data-parent") {
        bail!("An explicit temporary --data-parent is required");
    }
    let options = Options::parse(arguments)?;
    if options.environment != Environment::Dev {
        bail!("Drag fixtures require dev storage");
    }
    let paths = Storage::new(&options.data_parent, options.environment)?.open()?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let pool = db::open(&paths).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clipboard_items").fetch_one(&pool).await?;
        if count != 0 { bail!("Database already contains {count} records; refusing to reset it"); }
        let base = paths.state_dir().join("drag09");
        let sources = base.join("sources");
        std::fs::create_dir_all(&sources)?;
        for target in ["single", "multi", "image", "partial", "cancel", "missing"] {
            std::fs::create_dir_all(base.join("targets").join(target))?;
        }
        std::fs::create_dir_all(base.join("moved"))?;
        let single = sources.join("single.txt");
        let first = sources.join("alpha.txt");
        let second = sources.join("beta 中文.txt");
        std::fs::write(&single, "DRAG09-SINGLE-58F2\n")?;
        std::fs::write(&first, "DRAG09-MULTI-ALPHA-81C3\n")?;
        std::fs::write(&second, "DRAG09-MULTI-BETA-中文-92D4\n")?;
        let multi = format!("{}\n{}", first.display(), second.display());
        let name = format!("{}.png", "ef".repeat(32));
        let missing_name = format!("{}.png", "12".repeat(32));
        let original = paths.resources_dir().join("clipboard-images/origin/ef").join(&name);
        std::fs::create_dir_all(original.parent().unwrap())?;
        ImageBuffer::from_fn(4096, 2048, |x,y| {
            if x % 256 < 8 || y % 256 < 8 { Rgba([255u8,255,255,255]) }
            else { Rgba([((x/16)%256) as u8,((y/8)%256) as u8,180,255]) }
        }).save(&original)?;
        let thumbnail = paths.resources_dir().join("clipboard-images/thumbnails/12").join(&missing_name);
        std::fs::create_dir_all(thumbnail.parent().unwrap())?;
        image::RgbImage::new(300,150).save(thumbnail)?;
        let absent = sources.join("already-missing.txt");
        let fixtures = [
            ("drag09-single", "files", single.to_str().unwrap(), Some("f")),
            ("drag09-multi", "files", multi.as_str(), Some("f,f")),
            ("drag09-original", "image", name.as_str(), None),
            ("drag09-missing", "files", absent.to_str().unwrap(), Some("f")),
            ("drag09-image-missing", "image", missing_name.as_str(), None),
            ("drag09-paste", "text", "drag09-paste · DRAG09-CLICK-PASTE", None),
        ];
        let mut transaction = pool.begin().await?;
        for (index,(id,kind,content,types)) in fixtures.into_iter().enumerate() {
            sqlx::query("INSERT INTO clipboard_items (id,kind,content,content_hash,search_text,summary,note,file_types,width,height,size,platform,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,'macos',datetime('2026-11-18','+' || ? || ' seconds'),datetime('2026-11-18','+' || ? || ' seconds'))")
                .bind(id).bind(kind).bind(content).bind(id).bind((kind=="text").then_some(content)).bind((kind=="text").then_some(content)).bind(id).bind(types)
                .bind((id=="drag09-original").then_some(4096)).bind((id=="drag09-original").then_some(2048))
                .bind(if id=="drag09-original" { std::fs::metadata(&original)?.len() as i64 } else { content.len() as i64 }).bind(index as i64).bind(index as i64)
                .execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        pool.close().await;
        println!("Created 6 isolated drag records.\nDatabase: {}\nSources: {}\nEmpty Finder targets: {}\nOriginal PNG: {}", paths.db_path().display(), sources.display(), base.join("targets").display(), original.display());
        Ok(())
    })
}
