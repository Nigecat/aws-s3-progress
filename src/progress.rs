use aws_sdk_s3::client::customize::orchestrator::CustomizableOperation;
use aws_sdk_s3::primitives::SdkBody;
use aws_smithy_http::body::BoxBody;
use bytes::Bytes;
use http::{HeaderMap, Request};
use http_body::{Body, SizeHint};
use pin_project::pin_project;

use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// (data, chunk size (diff), total written, data size)
pub type HookFunction<T> = fn(&T, usize, u64, u64);

pub trait TrackableRequest {
    fn track<T>(self, data: T, hook: HookFunction<T>) -> Self
    where
        T: Send + Sync + 'static;
}

// ----------------------------------------------------------------------------------------

#[pin_project]
struct ProgressBody<I, T> {
    #[pin]
    inner: I,
    data: Arc<T>,
    hook: HookFunction<T>,
    written: u64,
    length: u64,
}

impl<I, T> ProgressBody<I, T>
where
    T: Send + Sync + 'static,
{
    fn new(inner: I, data: Arc<T>, hook: HookFunction<T>, length: u64) -> Self {
        ProgressBody {
            inner,
            data,
            hook,
            written: 0,
            length,
        }
    }

    fn patch(mut req: Request<SdkBody>, data: Arc<T>, hook: HookFunction<T>) -> Request<SdkBody> {
        // Extract current request body so we can modify it
        let body = mem::replace(req.body_mut(), SdkBody::taken()).map(move |body| {
            let len = body.content_length().unwrap_or(0);
            let body = ProgressBody::new(body, data.clone(), hook, len);
            SdkBody::from_dyn(BoxBody::new(body))
        });

        // Replace existing request body
        let _ = mem::replace(req.body_mut(), body);

        req
    }
}

impl<I, T> Body for ProgressBody<I, T>
where
    I: Body<Data = Bytes, Error = aws_smithy_http::body::Error>,
{
    type Data = Bytes;
    type Error = aws_smithy_http::body::Error;

    fn poll_data(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Self::Data, Self::Error>>> {
        let this = self.project();
        match this.inner.poll_data(cx) {
            Poll::Ready(Some(Ok(data))) => {
                *this.written += data.len() as u64;
                (this.hook)(&*this.data, data.len(), *this.written, *this.length);
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

// ----------------------------------------------------------------------------------------

impl<R, E, B> TrackableRequest for CustomizableOperation<R, E, B>
where
    R: Send,
    E: Send + Sync + std::error::Error + 'static,
    B: Send,
{
    fn track<T>(self, data: T, hook: HookFunction<T>) -> Self
    where
        T: Send + Sync + 'static,
    {
        let data = Arc::new(data);
        self.map_request(move |req| {
            Ok::<http::Request<aws_sdk_s3::primitives::SdkBody>, E>(ProgressBody::<(), T>::patch(
                req,
                data.clone(),
                hook,
            ))
        })
    }
}
