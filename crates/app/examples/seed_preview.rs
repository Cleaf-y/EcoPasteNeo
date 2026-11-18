//! Isolated preview fixtures. Explicit invocation only; refuses to reset existing data.
use anyhow::{Result, bail};
use ecopaste_neo_app::options::Options;
use ecopaste_neo_core::{
    db,
    paths::{Environment, Storage},
};
use image::{ImageBuffer, Rgba};

fn main() -> Result<()> {
    let arguments: Vec<_> = std::env::args().skip(1).collect();
    if !arguments.iter().any(|arg| arg == "--data-parent") {
        bail!("An explicit temporary --data-parent is required");
    }
    let options = Options::parse(arguments)?;
    if options.environment != Environment::Dev {
        bail!("Preview fixtures require dev storage");
    }
    let paths = Storage::new(&options.data_parent, options.environment)?.open()?;
    tokio::runtime::Runtime::new()?.block_on(async {
        let pool = db::open(&paths).await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM clipboard_items").fetch_one(&pool).await?;
        if count != 0 { bail!("Database already contains {count} records; refusing to reset it"); }
        let files = paths.state_dir().join("preview07");
        std::fs::create_dir_all(&files)?;
        let long = format!("preview07-long\n{}\nPREVIEW07-LONG-END-7F3A", (1..=180).map(|number| format!("第 {number:03} 行：完整历史预览。关闭预览后，完整内容仍应粘贴到原光标处。\n")).collect::<String>());
        std::fs::write(files.join("long-expected.txt"), &long)?;
        for name in ["move-me.txt", "second.txt", "third.txt"] { std::fs::write(files.join(name), "Isolated preview fixture\n")?; }
        let file_list = [files.join("move-me.txt"), files.join("second.txt"), files.join("third.txt"), files.clone(), files.join("long-expected.txt"), files.join("missing.pdf")].iter().map(|path| path.to_string_lossy().into_owned()).collect::<Vec<_>>().join("\n");
        let image_name = format!("{}.png", "ab".repeat(32));
        let image_path = paths.resources_dir().join("clipboard-images/origin/ab").join(&image_name);
        std::fs::create_dir_all(image_path.parent().unwrap())?;
        ImageBuffer::from_fn(4096, 2048, |x, y| {
            if x % 128 < 4 || y % 128 < 4 { Rgba([240u8, 240, 240, 255]) }
            else { Rgba([((x / 16) % 256) as u8, ((y / 8) % 256) as u8, 160, 255]) }
        }).save(&image_path)?;
        let fixtures = [
            ("preview07-short", "text", None, "preview07-short · SHORT-CURRENT-CONTENT", Some("preview07-short · SHORT-CURRENT-CONTENT"), false, "preview07-switch"),
            ("preview07-long", "text", None, long.as_str(), Some(long.as_str()), false, "preview07-long"),
            ("preview07-html", "text", Some("html"), "<p><b>preview07-html</b> HTML-PLAIN-END-48B2</p>", Some("preview07-html\nHTML-PLAIN-END-48B2"), false, "preview07-html"),
            ("preview07-rtf", "text", Some("rtf"), "{\\rtf1\\ansi preview07-rtf \\b RTF-PLAIN-END-93C1}", Some("preview07-rtf\nRTF-PLAIN-END-93C1"), false, "preview07-rtf"),
            ("preview07-image", "image", None, image_name.as_str(), None, false, "preview07-image preview07-switch"),
            ("preview07-files", "files", None, file_list.as_str(), None, false, "preview07-files"),
            ("preview07-sensitive", "text", None, "PREVIEW07-PRIVATE-FIXTURE", Some("preview07-sensitive PREVIEW07-PRIVATE-FIXTURE"), true, "preview07-sensitive"),
        ];
        let mut transaction = pool.begin().await?;
        for (index, (id, kind, sub_kind, content, plain, sensitive, note)) in fixtures.into_iter().enumerate() {
            sqlx::query("INSERT INTO clipboard_items (id,kind,sub_kind,content,content_hash,search_text,summary,file_types,width,height,size,is_sensitive,platform,note,created_at,updated_at) VALUES (?,?,?,?,?,?,?,?,?,?,?,?,'macos',?,datetime('2026-11-18','+' || ? || ' seconds'),datetime('2026-11-18','+' || ? || ' seconds'))")
                .bind(id).bind(kind).bind(sub_kind).bind(content).bind(id).bind(plain).bind(plain.map(|text| text.chars().take(512).collect::<String>()))
                .bind((kind == "files").then_some("f,f,f,d,f,f")).bind((kind == "image").then_some(4096)).bind((kind == "image").then_some(2048)).bind(if kind == "image" { std::fs::metadata(&image_path)?.len() as i64 } else { content.len() as i64 }).bind(sensitive).bind(note).bind(index as i64).bind(index as i64).execute(&mut *transaction).await?;
        }
        transaction.commit().await?;
        pool.close().await;
        println!("Created 7 isolated preview records.\nDatabase: {}\nExpected long text: {}\nMove for missing-file test: {}", paths.db_path().display(), files.join("long-expected.txt").display(), files.join("move-me.txt").display());
        Ok(())
    })
}
