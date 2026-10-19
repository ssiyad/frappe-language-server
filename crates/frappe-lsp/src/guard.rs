//! A bug in one request must not end the session.
//!
//! tower-lsp polls every handler inside its serve loop, so a panic anywhere in
//! a handler unwinds through the loop and the process exits: the editor loses
//! every feature until the user restarts it, for a fault in one hover.
//! [`Guarded`] answers that one request with an error instead, and the
//! [`Recover`] locks keep the next request from inheriting the panic.

use std::any::Any;
use std::panic::AssertUnwindSafe;
use std::sync::{PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::task::{Context, Poll};

use futures::FutureExt;
use futures::future::BoxFuture;
use tower::Service;
use tower_lsp_server::jsonrpc::{Error, ErrorCode, Request, Response};

/// Wraps the language server service so a panicking request is answered with
/// an internal error rather than taking the process down.
pub struct Guarded<S>(pub S);

impl<S> Service<Request> for Guarded<S>
where
    S: Service<Request, Response = Option<Response>>,
    S::Future: Send + 'static,
    S::Error: Send + 'static,
{
    type Response = Option<Response>;
    type Error = S::Error;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.0.poll_ready(cx)
    }

    fn call(&mut self, request: Request) -> Self::Future {
        let id = request.id().cloned();
        let method = request.method().to_string();
        let answer = move |payload: Box<dyn Any + Send>| {
            tracing::error!(method, panic = describe(&*payload), "a request panicked");
            Ok(id.map(|id| {
                Response::from_error(
                    id,
                    Error {
                        code: ErrorCode::InternalError,
                        message: format!("frappe-lsp: {method} failed; see the server log").into(),
                        data: None,
                    },
                )
            }))
        };

        match std::panic::catch_unwind(AssertUnwindSafe(|| self.0.call(request))) {
            Ok(future) => async move {
                match AssertUnwindSafe(future).catch_unwind().await {
                    Ok(result) => result,
                    Err(payload) => answer(payload),
                }
            }
            .boxed(),
            Err(payload) => async move { answer(payload) }.boxed(),
        }
    }
}

/// Locks that survive a panic in whoever held them last.
///
/// Only a writer poisons a lock. The data it leaves may be half-updated, but a
/// stale index answers the next request slightly wrong, and a poisoned one
/// answers no request ever again.
pub trait Recover<T> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T>;
    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T> Recover<T> for RwLock<T> {
    fn read_or_recover(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_or_recover(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

/// The message a panic carries, when it carries one.
fn describe(payload: &(dyn Any + Send)) -> &str {
    payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
        .unwrap_or("a panic with no message")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Panics while building its future, or while the future runs.
    struct Faulty {
        early: bool,
    }

    impl Service<Request> for Faulty {
        type Response = Option<Response>;
        type Error = ();
        type Future = BoxFuture<'static, Result<Option<Response>, ()>>;

        fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), ()>> {
            Poll::Ready(Ok(()))
        }

        fn call(&mut self, _: Request) -> Self::Future {
            assert!(!self.early, "a fault before the future exists");
            Box::pin(async { panic!("a fault inside the handler") })
        }
    }

    fn answer(early: bool, request: Request) -> Option<Response> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("a runtime");
        runtime
            .block_on(Guarded(Faulty { early }).call(request))
            .expect("answered")
    }

    #[test]
    fn a_panicking_request_is_answered_with_an_error() {
        for early in [false, true] {
            let request = Request::build("textDocument/hover").id(7).finish();
            let response = answer(early, request).expect("a request gets a response");

            assert_eq!(response.id(), &7.into());
            let error = response.error().expect("an error response");
            assert_eq!(error.code, ErrorCode::InternalError);
            assert!(error.message.contains("textDocument/hover"), "{error:?}");
        }
    }

    #[test]
    fn a_panicking_notification_is_answered_with_nothing() {
        let notification = Request::build("textDocument/didChange").finish();
        assert!(answer(false, notification).is_none());
    }

    #[test]
    fn a_lock_a_panic_poisoned_still_opens() {
        let lock = std::sync::Arc::new(RwLock::new(1));
        let held = std::sync::Arc::clone(&lock);
        let _ = std::thread::spawn(move || {
            let _guard = held.write_or_recover();
            panic!("while holding the lock");
        })
        .join();

        assert!(lock.is_poisoned());
        *lock.write_or_recover() += 1;
        assert_eq!(*lock.read_or_recover(), 2);
    }
}
