//! hook function: (chunk size (diff), total written, data size)

use std::convert::Infallible;
use std::mem;

use aws_sdk_s3::primitives::SdkBody;
use aws_smithy_http::body::BoxBody;
use bytes::Bytes;
use http::HeaderMap;
use http_body::{Body, SizeHint};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};

use aws_sdk_s3::operation::put_object::builders::PutObjectFluentBuilder;
use aws_sdk_s3::operation::put_object::PutObjectOutput;

#[pin_project]
struct ProgressBody<T, F> {
    #[pin]
    inner: T,
    hook: F,
    written: u64,
    length: u64,
}

impl<T, F> ProgressBody<T, F>
where
    F: 'static,
{
    fn new(inner: T, hook: F, length: u64) -> Self {
        ProgressBody {
            inner,
            hook,
            written: 0,
            length,
        }
    }
}

impl<T, F> Body for ProgressBody<T, F>
where
    T: Body<Data = Bytes, Error = aws_smithy_http::body::Error>,
    F: Fn(usize, u64, u64) -> () + Send + 'static,
{
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(data))) => {
                *this.written += data.len() as u64;
                (this.hook)(data.len(), *this.written, *this.length);
                Poll::Ready(Some(Ok(data)))
            }
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Pending => Poll::Pending,
        }
    }

    fn poll_trailers(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<HeaderMap>, Self::Error>> {
        self.project().inner.poll_trailers(cx)
    }

    fn size_hint(&self) -> http_body::SizeHint {
        SizeHint::with_exact(self.length)
    }
}

#[async_trait::async_trait]
pub trait TrackableRequest<R> {
    async fn send_tracked<F>(self, hook: F) -> Result<R, aws_sdk_s3::Error>
    where
        F: Fn(usize, u64, u64) -> () + Send + Sync + 'static;
}

// ----------------------------------------------------------------------------------------

#[async_trait::async_trait]
impl TrackableRequest<PutObjectOutput> for PutObjectFluentBuilder {
    async fn send_tracked<F>(self, hook: F) -> Result<PutObjectOutput, aws_sdk_s3::Error>
    where
        F: Fn(usize, u64, u64) -> () + Send + Sync + 'static,
    {
        Ok(self
            .customize()
            .await?
            .map_request::<_, Infallible>(|mut req| {
                // Extract current request body so we can modify it
                let body = mem::replace(req.body_mut(), SdkBody::taken()).map(move |body| {
                    let len = body.content_length().unwrap_or(0);
                    let body = ProgressBody::new(body, hook, len);
                    SdkBody::from_dyn(BoxBody::new(body))
                });

                // Replace existing request body
                let _ = mem::replace(req.body_mut(), body);

                Ok(req)
            })
            .send()
            .await?)
    }
}
