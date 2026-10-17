use crate::support::Fixture;
use aio_plugin_file_server::{FileService, migration::copy_legacy};
use anyhow::{Context, Result};
use sha2::{Digest, Sha256};
use std::{path::Path, sync::atomic::Ordering};

async fn legacy_file(
    f: &Fixture,
    root: &Path,
    name: &str,
    mime: &str,
    bytes: &[u8],
    token: Option<&str>,
) -> Result<(String, std::path::PathBuf)> {
    let id = uuid::Uuid::new_v4().to_string();
    let directory = root.join(format!("{:x}", Sha256::digest(b"tenant-a")));
    tokio::fs::create_dir_all(&directory).await?;
    let path = directory.join(format!("{id}.blob"));
    tokio::fs::write(&path, bytes).await?;
    sqlx::query("INSERT INTO public.file_objects VALUES($1,'tenant-a',$2,$3,$4,$5,$6,'original-author',$7,'2023-04-05T06:07:08.123456Z')")
        .bind(&id).bind(name).bind(mime).bind(bytes.len() as i64).bind(format!("{:x}",Sha256::digest(bytes))).bind(format!("{id}.blob")).bind(token).execute(&f.admin).await?;
    Ok((id, path))
}
async fn verify(f: &Fixture) -> Result<()> {
    sqlx::raw_sql("CREATE TABLE IF NOT EXISTS public.component_sources(id uuid PRIMARY KEY,git text UNIQUE NOT NULL); CREATE SCHEMA IF NOT EXISTS aio_plugin_host; CREATE TABLE IF NOT EXISTS aio_plugin_host.database_bindings(source_id text NOT NULL,tenant_id text NOT NULL,schema_name text NOT NULL,role_name text NOT NULL,PRIMARY KEY(source_id,tenant_id)); CREATE TABLE IF NOT EXISTS public.file_objects(id text PRIMARY KEY,tenant_id text NOT NULL,original_name text NOT NULL,content_type text NOT NULL,size_bytes bigint NOT NULL,sha256 text NOT NULL,storage_name text NOT NULL,uploaded_by text NOT NULL,image_token text,created_at timestamptz NOT NULL)").execute(&f.admin).await?;
    sqlx::query("INSERT INTO public.component_sources VALUES($1::text::uuid,'https://github.com/org-aio/aio-plugin-file.git') ON CONFLICT(git) DO UPDATE SET id=excluded.id")
        .bind(&f.source).execute(&f.admin).await?;
    sqlx::query("INSERT INTO aio_plugin_host.database_bindings VALUES($1,'tenant-a',$2,$3)")
        .bind(&f.source)
        .bind(&f.schema)
        .bind(&f.role)
        .execute(&f.admin)
        .await?;
    let temporary = tempfile::tempdir()?;
    let root = temporary.path();
    let token = "a".repeat(64);
    let svg = b"<svg xmlns='http://www.w3.org/2000/svg'/>";
    let (image, path) =
        legacy_file(f, root, "保留.svg", "image/svg+xml", svg, Some(&token)).await?;
    // 绑定表是唯一权威，错误绑定不能被猜测 schema 的逻辑绕过。
    sqlx::query(
        "UPDATE aio_plugin_host.database_bindings SET schema_name='p_bad' WHERE source_id=$1",
    )
    .bind(&f.source)
    .execute(&f.admin)
    .await?;
    assert!(
        copy_legacy(&f.admin, &f.admin, root, &f.service)
            .await
            .is_err()
    );
    assert!(f.service.list("tenant-a").await?.is_empty());
    sqlx::query("UPDATE aio_plugin_host.database_bindings SET schema_name=$2 WHERE source_id=$1")
        .bind(&f.source)
        .bind(&f.schema)
        .execute(&f.admin)
        .await?;
    tokio::fs::write(&path, vec![b'x'; svg.len()]).await?;
    assert!(
        copy_legacy(&f.admin, &f.admin, root, &f.service)
            .await
            .is_err()
    );
    let count: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM {}.file_objects", f.schema))
        .fetch_one(&f.admin)
        .await?;
    assert_eq!(count, 0, "损坏原件不能留下不可变上传意图");
    tokio::fs::write(&path, svg).await?;
    // 模拟对象已经写入，但确认回执丢失；重新运行必须续传同一 ID。
    f.objects.fail_write_reply.store(true, Ordering::SeqCst);
    assert!(
        copy_legacy(&f.admin, &f.admin, root, &f.service)
            .await
            .is_err()
    );
    assert!(f.service.list("tenant-a").await?.is_empty());
    let report = copy_legacy(&f.admin, &f.admin, root, &f.service).await?;
    assert_eq!((report.copied, report.verified), (1, 0));
    assert_eq!(report.links[0].old_path, format!("/i/{token}"));
    assert_eq!(
        report.links[0].new_path,
        format!("{}/i/{token}", f.public_base)
    );
    let listing = f.service.list("tenant-a").await?;
    let item = listing.first().context("迁移文件缺失")?;
    assert_eq!(item.id, image);
    assert_eq!(item.name, "保留.svg");
    assert_eq!(item.image_token.as_deref(), Some(token.as_str()));
    assert_eq!(item.uploaded_by, "original-author");
    assert_eq!(item.created_at, "2023-04-05T06:07:08.123456Z");
    assert_eq!(
        f.public(&format!("/i/{token}"))
            .send()
            .await?
            .bytes()
            .await?
            .as_ref(),
        svg
    );
    let bytes = vec![7; 5 * 1024 * 1024];
    let (binary, binary_path) =
        legacy_file(f, root, "old.bin", "application/octet-stream", &bytes, None).await?;
    let report = copy_legacy(&f.admin, &f.admin, root, &f.service).await?;
    assert_eq!((report.copied, report.verified), (1, 1));
    let report = copy_legacy(&f.admin, &f.admin, root, &f.service).await?;
    assert_eq!((report.copied, report.verified), (0, 2));
    // 冲突不会覆盖目标内容或修改旧文件。
    sqlx::query(&format!(
        "UPDATE {}.file_objects SET original_name='changed' WHERE id=$1",
        f.schema
    ))
    .bind(&binary)
    .execute(&f.admin)
    .await?;
    assert!(
        copy_legacy(&f.admin, &f.admin, root, &f.service)
            .await
            .is_err()
    );
    assert_eq!(tokio::fs::read(&path).await?, svg);
    assert_eq!(tokio::fs::read(&binary_path).await?, bytes);
    let old_name: String =
        sqlx::query_scalar("SELECT original_name FROM public.file_objects WHERE id=$1")
            .bind(&binary)
            .fetch_one(&f.admin)
            .await?;
    assert_eq!(old_name, "old.bin");
    sqlx::query("DELETE FROM public.file_objects WHERE id=$1 OR id=$2")
        .bind(image)
        .bind(binary)
        .execute(&f.admin)
        .await?;
    sqlx::query("DELETE FROM aio_plugin_host.database_bindings WHERE source_id=$1")
        .bind(&f.source)
        .execute(&f.admin)
        .await?;
    Ok(())
}
#[tokio::test]
#[ignore = "需要 AIO_FILE_TEST_DATABASE_URL 指向独立 PostgreSQL"]
async fn copies_legacy_files_without_replacing_identity_or_content() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = verify(&fixture).await;
    fixture.close().await?;
    result
}
