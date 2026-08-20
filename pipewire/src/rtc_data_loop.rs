// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

//! PipeWireAO real-time-control data loops.
//!
//! The C implementation owns thread creation, scheduling, idle behavior, and
//! lifecycle. This module only keeps a Rust process callback alive and exposes
//! the native loop through an idiomatic owner.

use std::{
    fmt, io,
    marker::PhantomData,
    os::raw::{c_int, c_void},
    ptr,
    rc::Rc,
};

use crate::{context::Context, loop_::Loop};

/// Behavior used when one RTC duty cycle reports no work.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcIdle {
    /// Poll again immediately. Requires a reserved physical core.
    BusySpin,
    /// Wait for a source registered on the RTC data loop.
    EventFd,
    /// Spin for a fixed number of iterations, then wait for a loop source.
    Hybrid { spin_iterations: u32 },
}

/// Scheduling policy requested from the context's `module-rt` implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtcScheduler {
    Other,
    /// `-1` selects the priority configured by `module-rt`.
    Fifo {
        priority: i32,
    },
}

/// Configuration for an [`RtcDataLoop`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtcDataLoopConfig {
    pub idle: RtcIdle,
    pub scheduler: RtcScheduler,
}

impl RtcDataLoopConfig {
    fn as_raw(self) -> pw_sys::pw_rtc_data_loop_config {
        let (idle, hybrid_spin_iterations) = match self.idle {
            RtcIdle::BusySpin => (
                pw_sys::pw_rtc_data_loop_idle_PW_RTC_DATA_LOOP_IDLE_BUSY_SPIN,
                0,
            ),
            RtcIdle::EventFd => (
                pw_sys::pw_rtc_data_loop_idle_PW_RTC_DATA_LOOP_IDLE_EVENTFD,
                0,
            ),
            RtcIdle::Hybrid { spin_iterations } => (
                pw_sys::pw_rtc_data_loop_idle_PW_RTC_DATA_LOOP_IDLE_HYBRID,
                spin_iterations,
            ),
        };
        let (scheduler, priority) = match self.scheduler {
            RtcScheduler::Other => (
                pw_sys::pw_rtc_data_loop_scheduler_PW_RTC_DATA_LOOP_SCHED_OTHER,
                0,
            ),
            RtcScheduler::Fifo { priority } => (
                pw_sys::pw_rtc_data_loop_scheduler_PW_RTC_DATA_LOOP_SCHED_FIFO,
                priority,
            ),
        };

        pw_sys::pw_rtc_data_loop_config {
            version: pw_sys::PW_VERSION_RTC_DATA_LOOP_CONFIG,
            idle,
            scheduler,
            priority,
            hybrid_spin_iterations,
        }
    }
}

/// Owns a native PipeWireAO RTC data loop.
///
/// The callback performs one bounded duty cycle and returns a positive work
/// count, zero when no work is available, or a negative errno-style result to
/// terminate the loop. It must not panic, allocate, perform file I/O, or wait
/// for a peer on the strict execution path.
///
/// A PipeWire main loop must continue running separately for protocol,
/// negotiation, topology, and control events.
pub struct RtcDataLoop<'context, F>
where
    F: FnMut() -> i32 + Send + 'context,
{
    ptr: ptr::NonNull<pw_sys::pw_rtc_data_loop>,
    process: Option<Box<F>>,
    not_send_or_sync: PhantomData<(&'context Context, Rc<()>)>,
}

impl<F> fmt::Debug for RtcDataLoop<'_, F>
where
    F: FnMut() -> i32 + Send,
{
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RtcDataLoop")
            .field("ptr", &self.ptr)
            .field("running", &self.is_running())
            .finish_non_exhaustive()
    }
}

impl<'context, F> RtcDataLoop<'context, F>
where
    F: FnMut() -> i32 + Send + 'context,
{
    /// Create an RTC data loop using the `ThreadUtils` installed on `context`.
    pub fn new(
        context: &'context Context,
        properties: Option<&spa::utils::dict::DictRef>,
        config: RtcDataLoopConfig,
        process: F,
    ) -> io::Result<Self> {
        unsafe extern "C" fn process_trampoline<F>(data: *mut c_void) -> c_int
        where
            F: FnMut() -> i32,
        {
            let process = unsafe { &mut *data.cast::<F>() };
            process()
        }

        let mut process = Box::new(process);
        let raw_config = config.as_raw();
        let properties = properties.map_or(ptr::null(), |value| value.as_raw_ptr());
        let raw = unsafe {
            pw_sys::pw_rtc_data_loop_new(
                context.as_raw_ptr(),
                properties,
                &raw_config,
                Some(process_trampoline::<F>),
                (&mut *process as *mut F).cast(),
            )
        };
        let ptr = ptr::NonNull::new(raw).ok_or_else(io::Error::last_os_error)?;

        Ok(Self {
            ptr,
            process: Some(process),
            not_send_or_sync: PhantomData,
        })
    }

    /// Return the native RTC data-loop pointer without transferring ownership.
    pub fn as_raw_ptr(&self) -> *mut pw_sys::pw_rtc_data_loop {
        self.ptr.as_ptr()
    }

    /// Get the event loop used by EventFd and Hybrid idle policies.
    pub fn loop_(&self) -> &Loop {
        let raw = unsafe { pw_sys::pw_rtc_data_loop_get_loop(self.as_raw_ptr()) };
        unsafe { &*raw.cast::<Loop>() }
    }

    /// Start the native RTC data loop.
    pub fn start(&mut self) -> io::Result<()> {
        result(unsafe { pw_sys::pw_rtc_data_loop_start(self.as_raw_ptr()) })
    }

    /// Stop and join the native RTC data loop.
    pub fn stop(&mut self) -> io::Result<()> {
        result(unsafe { pw_sys::pw_rtc_data_loop_stop(self.as_raw_ptr()) })
    }

    /// Request exit without joining.
    pub fn request_exit(&self) {
        unsafe { pw_sys::pw_rtc_data_loop_exit(self.as_raw_ptr()) }
    }

    pub fn is_running(&self) -> bool {
        unsafe { pw_sys::pw_rtc_data_loop_is_running(self.as_raw_ptr()) }
    }

    /// Return the terminal native result without changing the lifecycle.
    pub fn terminal_result(&self) -> i32 {
        unsafe { pw_sys::pw_rtc_data_loop_get_result(self.as_raw_ptr()) }
    }
}

impl<F> Drop for RtcDataLoop<'_, F>
where
    F: FnMut() -> i32 + Send,
{
    fn drop(&mut self) {
        unsafe {
            let _ = pw_sys::pw_rtc_data_loop_stop(self.as_raw_ptr());
            if pw_sys::pw_rtc_data_loop_get_thread(self.as_raw_ptr()).is_null() {
                pw_sys::pw_rtc_data_loop_destroy(self.as_raw_ptr());
            } else if let Some(process) = self.process.take() {
                // A failed join leaves the native thread able to call this
                // allocation. Leak both rather than create a use-after-free.
                std::mem::forget(process);
            }
        }
    }
}

fn result(value: i32) -> io::Result<()> {
    if value < 0 {
        Err(io::Error::from_raw_os_error(-value))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[allow(dead_code)]
    fn borrowed_process_is_supported<'a>(
        context: &'a Context,
        count: &'a AtomicU32,
    ) -> io::Result<RtcDataLoop<'a, impl FnMut() -> i32 + Send + 'a>> {
        RtcDataLoop::new(
            context,
            None,
            RtcDataLoopConfig {
                idle: RtcIdle::BusySpin,
                scheduler: RtcScheduler::Other,
            },
            move || {
                count.fetch_add(1, Ordering::Relaxed);
                1
            },
        )
    }

    #[test]
    fn native_config_preserves_idle_and_scheduler_choices() {
        let busy = RtcDataLoopConfig {
            idle: RtcIdle::BusySpin,
            scheduler: RtcScheduler::Other,
        }
        .as_raw();
        assert_eq!(
            busy.idle,
            pw_sys::pw_rtc_data_loop_idle_PW_RTC_DATA_LOOP_IDLE_BUSY_SPIN
        );
        assert_eq!(
            busy.scheduler,
            pw_sys::pw_rtc_data_loop_scheduler_PW_RTC_DATA_LOOP_SCHED_OTHER
        );
        assert_eq!(busy.priority, 0);
        assert_eq!(busy.hybrid_spin_iterations, 0);

        let hybrid = RtcDataLoopConfig {
            idle: RtcIdle::Hybrid {
                spin_iterations: 4096,
            },
            scheduler: RtcScheduler::Fifo { priority: 83 },
        }
        .as_raw();
        assert_eq!(
            hybrid.idle,
            pw_sys::pw_rtc_data_loop_idle_PW_RTC_DATA_LOOP_IDLE_HYBRID
        );
        assert_eq!(
            hybrid.scheduler,
            pw_sys::pw_rtc_data_loop_scheduler_PW_RTC_DATA_LOOP_SCHED_FIFO
        );
        assert_eq!(hybrid.priority, 83);
        assert_eq!(hybrid.hybrid_spin_iterations, 4096);
    }
}
