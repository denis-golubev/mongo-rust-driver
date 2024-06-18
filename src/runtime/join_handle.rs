use std::sync::{Arc, Mutex};
use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

/// A handle used for awaiting on tasks spawned in `AsyncRuntime::execute`.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug)]
pub(crate) struct AsyncJoinHandle<T>(tokio::task::JoinHandle<T>);

#[cfg(target_arch = "wasm32")]
#[derive(Debug)]
pub(crate) struct AsyncJoinHandle<T> {
    // Need to use Arc + Mutex here as several places in the driver
    // require a Send bound on the AsyncJoinHandle.
    result: std::sync::Arc<std::sync::Mutex<Option<T>>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl<T> AsyncJoinHandle<T> {
    pub(crate) fn spawn<F>(fut: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        #[cfg(not(feature = "sync"))]
        let handle = tokio::runtime::Handle::current();
        #[cfg(feature = "sync")]
        let handle = tokio::runtime::Handle::try_current()
            .unwrap_or_else(|_| crate::sync::TOKIO_RUNTIME.handle().clone());
        AsyncJoinHandle(handle.spawn(fut))
    }
}

// TODO: put this in sub-module and re-export the join_handle types here.
#[cfg(not(target_arch = "wasm32"))]
impl<T> Future for AsyncJoinHandle<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // Tokio wraps the task's return value with a `Result` that catches panics; in our case
        // we want to propagate the panic, so for once `unwrap` is the right tool to use.
        Pin::new(&mut self.0).poll(cx).map(|result| result.unwrap())
    }
}

#[cfg(target_arch = "wasm32")]
impl<T> AsyncJoinHandle<T> {
    pub(crate) fn spawn<F>(fut: F) -> Self
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let join_handle = AsyncJoinHandle {
            result: Arc::new(Mutex::new(None.into())),
        };

        let result_for_spawn = join_handle.result.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let result = fut.await;
            // If the Mutex was poisoned somehow, then all bets are off already.
            let mut result_for_spawn = result_for_spawn.lock().unwrap();
            *result_for_spawn = Some(result);
        });

        join_handle
    }
}

#[cfg(target_arch = "wasm32")]
impl<T> Future for AsyncJoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut result = self.result.lock().unwrap();

        if result.is_none() {
            return Poll::Pending;
        }

        // We already checked if there's a value, so the unwrap is safe.
        return Poll::Ready(result.take().unwrap());
    }
}
