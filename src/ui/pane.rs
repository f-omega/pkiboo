use async_trait::async_trait;
use std::error::Error;
use std::future::Future;

/// A presentation-only grouping of structured output.
///
/// Unlike a task, a pane has no running, completed, failed, or cancelled
/// state. Backends may render it as a terminal section, GUI card, panel, or
/// tab.
#[async_trait(?Send)]
pub trait Pane: super::Presenter + Clone {
    /// Signal that no more output will be added to this pane.
    async fn finish(&self);
}

/// Something that can create presentation panes.
///
/// Both root UIs and tasks can group output without pretending that rendering
/// the output is itself an operation with progress.
#[async_trait(?Send)]
pub trait PaneStarter {
    type PaneHandle: Pane;

    async fn start_pane(&self, title: String) -> Self::PaneHandle;
}

/// Convenience operations available to every pane starter.
#[async_trait(?Send)]
pub trait PaneStarterExt: PaneStarter {
    async fn pane<A, R, Operation>(
        &self,
        title: String,
        operation: Operation,
    ) -> Result<A, Box<dyn Error>>
    where
        R: Future<Output = Result<A, Box<dyn Error>>>,
        Operation: FnOnce(Self::PaneHandle) -> R;
}

#[async_trait(?Send)]
impl<T: PaneStarter> PaneStarterExt for T {
    async fn pane<A, R, Operation>(
        &self,
        title: String,
        operation: Operation,
    ) -> Result<A, Box<dyn Error>>
    where
        R: Future<Output = Result<A, Box<dyn Error>>>,
        Operation: FnOnce(Self::PaneHandle) -> R,
    {
        let pane = self.start_pane(title).await;
        let result = operation(pane.clone()).await;
        pane.finish().await;
        result
    }
}
