use futures::{Future, Stream};
use pin_project::{pin_project, pinned_drop};
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project(PinnedDrop)]
pub struct DropStream<S, F, Fut>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    #[pin]
    stream: S,
    on_drop: Option<F>,
}

impl<S, F, Fut> DropStream<S, F, Fut>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    pub fn new(stream: S, on_drop: F) -> Self {
        Self {
            stream,
            on_drop: Some(on_drop),
        }
    }
}

impl<S, F, Fut> Stream for DropStream<S, F, Fut>
where
    S: Stream,
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.stream.poll_next(cx)
    }
}

#[pinned_drop]
impl<S, F, Fut> PinnedDrop for DropStream<S, F, Fut>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn drop(self: Pin<&mut Self>) {
        if let Some(f) = self.project().on_drop.take() {
            let fut = f();
            tokio::spawn(fut);
        }
    }
}
