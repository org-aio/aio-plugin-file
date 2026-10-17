mod support;
use aio_plugin_file_model::{
    BeginUpload, FILE_CHUNK_BYTES, FileItem, FileListing, FileResponse, UploadSession,
};
use aio_plugin_file_server::{FileQuery, FileService};
use anyhow::{Context, Result};
use axum::body::Bytes;
use reqwest::{Method, StatusCode};
use sha2::{Digest, Sha256};
use std::sync::atomic::Ordering;
use support::Fixture;

fn query(id: &str) -> FileQuery {
    FileQuery {
        tenant_id: "tenant-a".into(),
        file_id: id.into(),
    }
}
fn declaration(name: &str, size: u64) -> BeginUpload {
    BeginUpload {
        name: name.into(),
        content_type: "image/png".into(),
        size_bytes: size,
    }
}

async fn large_file(f: &Fixture) -> Result<()> {
    let response = f
        .request(Method::POST, "/uploads", "owner")
        .json(&declaration("大图片.png", 100 * 1024 * 1024))
        .send()
        .await?;
    assert_eq!(response.status(), StatusCode::CREATED);
    let upload = response.json::<FileResponse<UploadSession>>().await?.data;
    assert_eq!(upload.chunk_count, 25);
    assert_eq!(upload.chunk_bytes, FILE_CHUNK_BYTES);
    assert!(f.service.list("tenant-a").await?.is_empty());
    let incomplete = f
        .request(
            Method::POST,
            &format!("/uploads/{}/complete", upload.id),
            "owner",
        )
        .send()
        .await?;
    assert_eq!(incomplete.status(), StatusCode::CONFLICT);
    assert_eq!(
        f.request(
            Method::GET,
            &format!("/uploads/{}", upload.id),
            "other-user"
        )
        .send()
        .await?
        .status(),
        StatusCode::NOT_FOUND
    );
    let mut digest = Sha256::new();
    for index in 0..upload.chunk_count {
        let bytes = vec![index as u8; FILE_CHUNK_BYTES as usize];
        digest.update(&bytes);
        let response = f
            .request(
                Method::PUT,
                &format!("/uploads/{}/chunks/{index}", upload.id),
                "owner",
            )
            .body(bytes)
            .send()
            .await?;
        assert_eq!(
            response.status(),
            StatusCode::NO_CONTENT,
            "{}",
            response.text().await?
        );
    }
    let state = f
        .request(Method::GET, &format!("/uploads/{}", upload.id), "owner")
        .send()
        .await?
        .json::<FileResponse<UploadSession>>()
        .await?
        .data;
    assert_eq!(state.received, (0..25).collect::<Vec<_>>());
    assert_eq!(
        f.request(
            Method::PUT,
            &format!("/uploads/{}/chunks/25", upload.id),
            "owner"
        )
        .body("bad")
        .send()
        .await?
        .status(),
        StatusCode::BAD_REQUEST
    );
    let item = f
        .request(
            Method::POST,
            &format!("/uploads/{}/complete", upload.id),
            "owner",
        )
        .send()
        .await?
        .error_for_status()?
        .json::<FileResponse<FileItem>>()
        .await?
        .data;
    let expected = format!("{:x}", digest.finalize());
    assert_eq!(item.sha256, expected);
    assert_eq!(item, f.service.complete(query(&item.id), "owner").await?);
    let token = item.image_token.as_ref().context("图片缺少公开令牌")?;
    assert_eq!(
        item.public_path,
        Some(format!("{}/i/{token}", f.public_base))
    );
    for public in [false, true] {
        let request = if public {
            f.public(&format!("/i/{token}"))
        } else {
            f.request(Method::GET, &format!("/files/{}", item.id), "owner")
        };
        let mut response = request.send().await?.error_for_status()?;
        assert_eq!(response.content_length(), Some(100 * 1024 * 1024));
        assert_eq!(response.headers()["cache-control"], "no-store");
        let mut actual = Sha256::new();
        let mut count = 0;
        while let Some(bytes) = response.chunk().await? {
            count += bytes.len();
            actual.update(&bytes);
        }
        assert_eq!(count, 100 * 1024 * 1024);
        assert_eq!(format!("{:x}", actual.finalize()), expected);
    }
    let path = format!("/i/{token}");
    assert_eq!(
        f.public(&path)
            .header("x-aio-tenant-id", "tenant-b")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.public(&path)
            .header("x-aio-user-id", "owner")
            .send()
            .await?
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        f.public("/files").send().await?.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.client
            .get(format!("{}/files", f.origin))
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        f.request(Method::GET, "/files", "owner")
            .header("x-aio-tenant-id", "tenant-b")
            .send()
            .await?
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let mut foreign = f.request(Method::GET, "/files", "owner").build()?;
    foreign
        .headers_mut()
        .insert("x-aio-tenant-id", "tenant-b".parse()?);
    assert_eq!(
        f.client.execute(foreign).await?.status(),
        StatusCode::FORBIDDEN
    );
    let listing = f
        .request(Method::GET, "/files", "owner")
        .send()
        .await?
        .json::<FileResponse<FileListing>>()
        .await?
        .data;
    assert_eq!(listing.files, vec![item.clone()]);
    let reopened = f.reopened()?;
    reopened.verify_host_database().await?;
    assert_eq!(reopened.list("tenant-a").await?, vec![item.clone()]);
    reopened.shutdown().await;
    // 首块被破坏时即拒绝内容，不把错误对象作为完整下载交给浏览器。
    let key = format!("files/{}/0", item.id);
    f.objects
        .values
        .lock()
        .map_err(|_| anyhow::anyhow!("对象锁失败"))?
        .insert(key.clone(), vec![9; FILE_CHUNK_BYTES as usize]);
    if let Ok(response) = f
        .request(Method::GET, &format!("/files/{}", item.id), "owner")
        .send()
        .await
    {
        assert!(response.bytes().await.is_err());
    }
    f.objects
        .values
        .lock()
        .map_err(|_| anyhow::anyhow!("对象锁失败"))?
        .insert(key, vec![0; FILE_CHUNK_BYTES as usize]);
    assert!(f.service.delete(query(&item.id)).await?);
    assert_eq!(
        f.public(&path).send().await?.status(),
        StatusCode::NOT_FOUND
    );
    assert!(f.service.download(query(&item.id)).await?.is_none());
    assert_eq!(f.service.cleanup().await?, 0, "删除隔离期不能提前回收对象");
    f.ready_cleanup(&item.id).await?;
    f.objects.fail_remove.store(true, Ordering::SeqCst);
    assert!(f.service.cleanup().await.is_err());
    let retained: i64 = sqlx::query_scalar(&format!(
        "SELECT count(*) FROM {}.file_objects WHERE id=$1 AND state='deleting'",
        f.schema
    ))
    .bind(&item.id)
    .fetch_one(&f.admin)
    .await?;
    assert_eq!(retained, 1);
    f.objects.fail_remove.store(false, Ordering::SeqCst);
    let reopened = f.reopened()?;
    assert_eq!(reopened.cleanup().await?, 1);
    reopened.shutdown().await;
    assert!(
        f.objects
            .values
            .lock()
            .map_err(|_| anyhow::anyhow!("对象锁失败"))?
            .is_empty()
    );
    Ok(())
}

async fn interrupted_uploads(f: &Fixture) -> Result<()> {
    let upload = f
        .service
        .begin("tenant-a", "owner", declaration("重试.png", 4))
        .await?;
    f.objects.fail_write_reply.store(true, Ordering::SeqCst);
    assert!(
        f.service
            .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"same"))
            .await
            .is_err()
    );
    assert!(
        f.service
            .upload_status(query(&upload.id), "owner")
            .await?
            .received
            .is_empty()
    );
    assert!(
        f.service
            .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"diff"))
            .await
            .is_err()
    );
    let reopened = f.reopened()?;
    reopened
        .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"same"))
        .await?;
    let item = reopened.complete(query(&upload.id), "owner").await?;
    assert_eq!(item.sha256, format!("{:x}", Sha256::digest(b"same")));
    reopened.shutdown().await;
    assert!(f.service.delete(query(&item.id)).await?);
    f.ready_cleanup(&item.id).await?;
    f.service.cleanup().await?;

    let upload = f
        .service
        .begin("tenant-a", "owner", declaration("取消.png", 4))
        .await?;
    let service = f.service.clone();
    let id = upload.id.clone();
    f.objects.hold_write.store(true, Ordering::SeqCst);
    let writing = tokio::spawn(async move {
        service
            .put_chunk(query(&id), "owner", 0, Bytes::from_static(b"late"))
            .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        f.objects.writing.notified(),
    )
    .await?;
    writing.abort();
    assert!(writing.await.is_err());
    assert!(f.service.cancel_upload(query(&upload.id), "owner").await?);
    assert_eq!(f.service.cleanup().await?, 0);
    f.objects.release.notify_one();
    assert!(
        f.service
            .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"late"))
            .await
            .is_err()
    );
    f.ready_cleanup(&upload.id).await?;
    let reopened = f.reopened()?;
    assert_eq!(reopened.cleanup().await?, 1);
    reopened.shutdown().await;
    assert!(
        f.objects
            .values
            .lock()
            .map_err(|_| anyhow::anyhow!("对象锁失败"))?
            .is_empty()
    );

    let upload = f
        .service
        .begin("tenant-a", "owner", declaration("到期.png", 4))
        .await?;
    f.service
        .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"expi"))
        .await?;
    sqlx::query(&format!(
        "UPDATE {}.file_objects SET expires_at=now()-interval '1 second' WHERE id=$1",
        f.schema
    ))
    .bind(&upload.id)
    .execute(&f.admin)
    .await?;
    assert!(
        f.service
            .complete(query(&upload.id), "owner")
            .await
            .is_err()
    );
    assert_eq!(f.service.cleanup().await?, 0);
    f.ready_cleanup(&upload.id).await?;
    assert_eq!(f.service.cleanup().await?, 1);
    Ok(())
}

async fn limits_and_races(f: &Fixture) -> Result<()> {
    assert!(
        f.service
            .begin(
                "tenant-a",
                "owner",
                declaration("oversize", 100 * 1024 * 1024 + 1)
            )
            .await
            .is_err()
    );
    assert!(
        f.service
            .begin("tenant-b", "owner", declaration("foreign", 4))
            .await
            .is_err()
    );
    let empty = f
        .service
        .begin("tenant-a", "owner", declaration("empty.png", 0))
        .await?;
    let item = f.service.complete(query(&empty.id), "owner").await?;
    assert_eq!(item.sha256, format!("{:x}", Sha256::digest([])));
    assert_eq!(
        f.request(Method::GET, &format!("/files/{}", item.id), "owner")
            .send()
            .await?
            .bytes()
            .await?
            .len(),
        0
    );
    let upload = f
        .service
        .begin("tenant-a", "owner", declaration("race.png", 4))
        .await?;
    let (a, b) = tokio::join!(
        f.service
            .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"aaaa")),
        f.service
            .put_chunk(query(&upload.id), "owner", 0, Bytes::from_static(b"bbbb"))
    );
    assert_ne!(a.is_ok(), b.is_ok());
    let item = f.service.complete(query(&upload.id), "owner").await?;
    let expected = if a.is_ok() { b"aaaa" } else { b"bbbb" };
    assert_eq!(item.sha256, format!("{:x}", Sha256::digest(expected)));
    assert!(!f.service.cancel_upload(query(&upload.id), "other").await?);
    let mut uploads = Vec::new();
    for _ in 0..8 {
        uploads.push(
            f.service
                .begin("tenant-a", "limited", declaration("pending", 1))
                .await?,
        );
    }
    assert!(
        f.service
            .begin("tenant-a", "limited", declaration("over-limit", 1))
            .await
            .is_err()
    );
    for upload in uploads {
        assert!(
            f.service
                .cancel_upload(query(&upload.id), "limited")
                .await?
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "需要 AIO_FILE_TEST_DATABASE_URL 指向独立 PostgreSQL"]
async fn persistent_chunks_streams_and_recovery() -> Result<()> {
    let fixture = Fixture::new().await?;
    let result = tokio::time::timeout(std::time::Duration::from_secs(180), async {
        large_file(&fixture).await?;
        interrupted_uploads(&fixture).await?;
        limits_and_races(&fixture).await?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .context("文件集成验收超时")?;
    fixture.close().await?;
    result
}

#[path = "support/migration.rs"]
mod migration;
