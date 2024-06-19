// TODO: Clean up imports, add explicit wasm_bindgen and js_sys dependencies

use std::future::Future;
use std::pin::Pin;
// TODO: here as well as at the spawn impl, consider using tokio's Mutex?
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll};

use chrono::{DateTime, TimeDelta, Utc};
use wasm_bindgen_futures::wasm_bindgen::closure::Closure;
use web_sys::wasm_bindgen::JsCast;

struct SleepData {
    done: bool,
    interval_id: Option<i32>,
}

pub(crate) struct Sleep {
    data: Arc<Mutex<SleepData>>,
}

impl Future for Sleep {
    type Output = ();

    fn poll(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Self::Output> {
        let done = self.data.lock().unwrap();
        if done.done {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for Sleep {
    fn drop(&mut self) {
        let mut data = self.data.lock().unwrap();
        if let Some(interval_id) = data.interval_id {
            let window = web_sys::window().expect("no global `window` exists");
            window.clear_interval_with_handle(interval_id);
        }
    }
}

pub(crate) async fn sleep(duration: TimeDelta) {
    let sleep = Sleep {
        data: Arc::new(Mutex::new(SleepData {
            done: false,
            interval_id: None,
        })),
    };

    let data_for_interval = sleep.data.clone();

    // Other than the `done`, everything below is only needed temporary, thus
    // a block is introduced to ensure these items are dropped before the
    // await boundary below to allow the resulting Future to be Send :)
    {
        let window = web_sys::window().expect("no global `window` exists");

        let interval_fn = move || {
            let mut data = data_for_interval.lock().unwrap();
            data.done = true;
        };
        let interval_closure = Closure::<dyn Fn()>::new(interval_fn);
        let interval_id = window
            .set_interval_with_callback_and_timeout_and_arguments_0(
                // TODO: The unchecked_ref may cause runtime issues, but
                //       that's the example given in
                //       https://rustwasm.github.io/docs/wasm-bindgen/examples/closures.html
                interval_closure.as_ref().unchecked_ref(),
                duration.num_milliseconds() as i32,
            )
            .expect("failed to set interval");

        let mut data = sleep.data.lock().unwrap();
        data.interval_id = Some(interval_id);
    }

    sleep.await
}

pub(crate) struct Interval {
    duration: TimeDelta,
    // TODO: The precision is not the best here...
    last_tick: Option<DateTime<Utc>>,
}

impl Interval {
    // TODO: This is not compatible with tokio's Interval as
    //       it does not have all the other fancy utility methods
    pub(crate) async fn tick(&mut self) {
        if let Some(mut last_tick) = self.last_tick {
            let now = Utc::now();
            let elapsed = now - last_tick;
            if elapsed < self.duration {
                sleep(self.duration - elapsed).await;
                // Only consume the tick if the sleep was successful
                // => this should make it cancel safe, I think.
                // TODO: read about tokio's cancellation safety
                last_tick = now;
            }
        } else {
            // To be compatible with tokio's interval, which
            // produces the first tick immediately.
            return;
        }
    }
}

pub(crate) fn interval(duration: TimeDelta) -> Interval {
    Interval {
        duration,
        last_tick: None,
    }
}
