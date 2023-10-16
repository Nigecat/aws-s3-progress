use aws_s3_progress::TrackableRequest;
use aws_sdk_s3::primitives::ByteStream;

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), aws_sdk_s3::Error> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_s3::Client::new(&config);

    // NOTE: The frequency the callback is called at is determined by the chunk_size of the given ByteStream
    let body = ByteStream::default();
    let _response = client
        .put_object()
        .bucket("bucket")
        .key("key")
        .body(body)
        // ----------------
        .customize() // internally, this function is a) not-async and b) always returns Ok(), so the next line is somewhat redundant
        .await?
        .track(0, |data, chunk, current, total| {
            println!("{current}/{total} ({chunk}) | DATA: ${data}")
        })
        // ----------------
        .send()
        .await?;

    Ok(())
}
