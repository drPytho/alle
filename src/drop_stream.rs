use futures::Stream;
use pin_project::{pin_project, pinned_drop};
use std::pin::Pin;
use std::task::{Context, Poll};

#[pin_project(PinnedDrop)]
pub struct DropStream<S, F>
where
    F: FnOnce(),
{
    #[pin]
    stream: S,
    on_drop: Option<F>,
}

impl<S, F> DropStream<S, F>
where
    F: FnOnce(),
{
    pub fn new(stream: S, on_drop: F) -> Self {
        Self {
            stream,
            on_drop: Some(on_drop),
        }
    }
}

impl<S, F> Stream for DropStream<S, F>
where
    S: Stream,
    F: FnOnce(),
{
    type Item = S::Item;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.project();
        this.stream.poll_next(cx)
    }
}

#[pinned_drop]
impl<S, F: FnOnce()> PinnedDrop for DropStream<S, F> {
    fn drop(self: Pin<&mut Self>) {
        if let Some(f) = self.project().on_drop.take() {
            f();
        }
    }
}
