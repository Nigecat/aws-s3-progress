mod progress;

use aws_sdk_s3::primitives::ByteStream;
use progress::TrackableRequest;

#[tokio::main]
#[allow(clippy::result_large_err)]
async fn main() -> Result<(), aws_sdk_s3::Error> {
    let config = aws_config::load_from_env().await;
    let client = aws_sdk_s3::Client::new(&config);

    let body = ByteStream::default();
    let _response = client
        .put_object()
        .bucket("bucket")
        .key("key")
        .body(body)
        // ----------------
        .customize() // internally, this function is a) not-async and b) always returns Ok(), so the next two lines are somewhat redundant
        .await
        .unwrap()
        .track(|chunk, current, total| println!("{current}/{total} ({chunk})"))
        // ----------------
        .send()
        .await
        .unwrap();

    Ok(())
}
