// Copyright The pipewire-rs Contributors.
// SPDX-License-Identifier: MIT

use std::{
    cell::Cell,
    ffi,
    ffi::{CStr, CString},
    io,
    ops::Deref,
    os::fd::{BorrowedFd, RawFd},
    pin::Pin,
    ptr,
    rc::{Rc, Weak},
    sync::Arc,
    time::{Duration, Instant},
};

use crate::{
    buffer::{Buffer, RetainedFilterBufferRc},
    core::CoreRc,
    properties::PropertiesBox,
    Error,
};
use spa::buffer::{
    meta::{MetaAcquisition, Metadata},
    Data,
};
use spa::utils::result::SpaResult;

use super::{
    Filter, FilterBox, FilterPortFlags, FilterPortRef, FilterState, ListenerLocalCallbacks,
};

/// SPA I/O identifier for PipeWire's graph-independent latest-buffer area.
///
/// This extension uses the ABI value assigned after `SPA_IO_AsyncBuffers` and is
/// available when both endpoints and the daemon advertise latest-buffer I/O.
pub const BUFFER_LATEST_IO_ID: u32 = 11;

/// SPA I/O identifier for the process-local latest-buffer notification fd.
pub const BUFFER_LATEST_NOTIFY_IO_ID: u32 = 12;

/// SPA I/O identifier for one process-local latest-buffer link descriptor.
pub const BUFFER_LATEST_LINK_IO_ID: u32 = 13;

/// Input-port property used to select latest-buffer receiver waiting.
pub const BUFFER_LATEST_WAIT_PROPERTY: &str = "port.buffer-latest.wait";

/// Empty spin iterations between monotonic deadline checks.
///
/// BusySpin and Hybrid use an iteration bound instead of reading the clock on
/// every empty shared-state poll. The first empty poll checks the deadline;
/// subsequent checks are separated by at most this many empty polls.
pub const BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL: u32 = 256;

/// Snapshot of one latest-buffer link change delivered by `io_changed`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct BufferLatestLinkInfo {
    /// Process-local link/mix identifier.
    pub id: u32,
    /// Whether this descriptor installs or retires the link.
    pub active: bool,
    /// Advisory notification descriptor, when negotiated for this link.
    pub notification_fd: Option<RawFd>,
}

impl BufferLatestLinkInfo {
    /// Decodes a latest-buffer link descriptor from an `io_changed` callback.
    ///
    /// Returns `None` for another I/O type, a missing descriptor, or an invalid
    /// descriptor. The shared mailbox pointer deliberately remains private;
    /// applications operate it through [`FilterBufferLatestPort`].
    ///
    /// # Safety
    ///
    /// `area` must be null or point to at least `size` readable bytes for the
    /// duration of the callback. The returned value is an immediate snapshot
    /// and does not borrow the descriptor.
    pub unsafe fn from_io_changed(io_id: u32, area: *const ffi::c_void, size: u32) -> Option<Self> {
        if io_id != BUFFER_LATEST_LINK_IO_ID
            || area.is_null()
            || (size as usize) < std::mem::size_of::<spa_sys::spa_io_buffers_latest_link>()
        {
            return None;
        }
        let raw = area.cast::<spa_sys::spa_io_buffers_latest_link>().read();
        const ACTIVE: u32 = 1;
        if raw.reserved != 0
            || raw.flags & !ACTIVE != 0
            || raw.notify_fd < -1
            || (raw.flags & ACTIVE != 0 && raw.io.is_null())
        {
            return None;
        }
        Some(Self {
            id: raw.id,
            active: raw.flags & ACTIVE != 0,
            notification_fd: (raw.notify_fd >= 0).then_some(raw.notify_fd),
        })
    }
}

/// Receiver-side waiting policy for a graph-independent latest-buffer link.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BufferLatestWaitPolicy {
    /// Poll shared state continuously without allocating or signaling an fd.
    BusySpin,
    /// Recheck shared state around a blocking eventfd wait.
    EventFd,
    /// Poll shared state for a bounded number of iterations, then use eventfd.
    Hybrid { spin_iterations: u32 },
}

/// Producer-local accounting for bounded latest-buffer acquisition.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BufferLatestStats {
    /// Output acquisition duty cycles.
    pub dequeue_attempts: u64,
    /// Returned consumer leases examined.
    pub recycle_returns: u64,
    /// Reusable pool slots examined.
    pub buffer_probes: u64,
    /// Attempts that found no safely reusable allocation.
    pub pool_exhaustions: u64,
    /// Unclaimed publications reclaimed after a full scan.
    pub ready_reclaims: u64,
    /// Subscriber ready slots withdrawn while reclaiming a pool buffer.
    pub ready_withdrawals: u64,
    /// Output buffers offered to the active fan-out set.
    pub publications: u64,
    /// Active subscriber mailboxes visited by publication.
    pub subscriber_visits: u64,
    /// Subscriber-local leases created by publication.
    pub subscriber_deliveries: u64,
    /// Subscriber-local ready IDs replaced by a newer publication.
    pub subscriber_supersessions: u64,
    /// Retired subscriber slots acknowledged by the producer.
    pub subscriber_retirements: u64,
    /// Outstanding subscriber leases recovered during retirement.
    pub retired_leases: u64,
    /// Publications that raced with removal and reached no active subscriber.
    pub zero_recipient_publications: u64,
    /// Largest number of pool slots examined by one scan.
    pub max_buffer_probes: u32,
    /// Largest aggregate recycle drain in one acquisition attempt.
    pub max_recycle_returns: u32,
    /// Largest number of ready mailboxes withdrawn in one reclaim attempt.
    pub max_ready_withdrawals: u32,
    /// Largest active fan-out visited by one publication.
    pub max_subscriber_visits: u32,
}

/// Maximum number of positions in one prepared complete-buffer rendezvous.
pub const MAX_RENDEZVOUS_INPUTS: usize = 64;

/// Release behavior for one explicitly prepared complete-buffer rendezvous.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RendezvousReleasePolicy {
    /// Release as soon as every required input is present, or at the deadline.
    CompleteOrDeadline,
    /// Release only at the first decision at or after the configured time.
    FixedRelease,
}

/// Event that made one complete-buffer rendezvous result eligible.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RendezvousReleaseCause {
    /// Every required input arrived before the absolute release time.
    Complete,
    /// An incomplete complete-or-deadline acquisition reached its release time.
    Deadline,
    /// A fixed-phase acquisition reached its release time.
    FixedRelease,
}

/// One immutable complete-buffer rendezvous decision.
#[derive(Clone, Copy, Debug)]
pub struct RendezvousRelease {
    /// Complete expected acquisition metadata copied when the acquisition began.
    pub acquisition: MetaAcquisition,
    /// Bit set for every position whose matching buffer lease was retained.
    pub accepted_inputs: u64,
    /// Required positions without a matching buffer at release.
    pub missing_required_inputs: u64,
    /// Event that made this result eligible.
    pub cause: RendezvousReleaseCause,
}

/// Fixed single-writer accounting for one complete-buffer rendezvous.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RendezvousStats {
    /// Matching observations retained in their input positions.
    pub accepted: u64,
    /// Reserved for duplicate observations suppressed by retained-position gating.
    pub duplicate: u64,
    /// Observations that preceded the active acquisition.
    pub stale: u64,
    /// Observations that followed the active acquisition.
    pub future: u64,
    /// Invalid, progressive, or different-domain observations.
    pub rejected: u64,
    /// Early releases with every required input present.
    pub complete_releases: u64,
    /// Complete-or-deadline releases at the absolute release time.
    pub deadline_releases: u64,
    /// Fixed-phase releases at the absolute release time.
    pub fixed_releases: u64,
    /// Sum of missing required positions over all release decisions.
    pub missing_required_inputs: u64,
    /// Buffer leases returned to their input ports.
    pub lease_returns: u64,
    /// Failed lease-return attempts.
    pub cleanup_errors: u64,
}

impl BufferLatestWaitPolicy {
    /// Value to set on [`BUFFER_LATEST_WAIT_PROPERTY`] before connecting.
    pub const fn port_property_value(self) -> &'static str {
        match self {
            Self::BusySpin => "busy-spin",
            Self::EventFd => "eventfd",
            Self::Hybrid { .. } => "hybrid",
        }
    }

    /// Configures port properties for this policy before the port is added.
    pub fn configure_port(self, properties: &mut crate::properties::Properties) {
        properties.insert(BUFFER_LATEST_WAIT_PROPERTY, self.port_property_value());
    }

    const fn notification_spin_iterations(self) -> Option<u32> {
        match self {
            Self::BusySpin => None,
            Self::EventFd => Some(0),
            Self::Hybrid { spin_iterations } => Some(spin_iterations),
        }
    }
}

#[derive(Debug)]
struct FilterRcInner {
    filter: FilterBox<'static>,
    _core: CoreRc,
}

/// Reference-counted smart pointer providing shared ownership of a PipeWire
/// [filter](super).
#[derive(Clone, Debug)]
pub struct FilterRc {
    inner: Rc<FilterRcInner>,
}

impl FilterRc {
    /// Creates an unconnected filter while retaining its core.
    pub fn new(core: CoreRc, name: &str, properties: PropertiesBox) -> Result<Self, Error> {
        let name = CString::new(name).expect("Invalid byte in filter name");
        Self::new_cstr(core, &name, properties)
    }

    /// Creates an unconnected filter with a C string name.
    pub fn new_cstr(core: CoreRc, name: &CStr, properties: PropertiesBox) -> Result<Self, Error> {
        unsafe {
            let filter =
                pw_sys::pw_filter_new(core.as_raw_ptr(), name.as_ptr(), properties.into_raw());
            let filter = ptr::NonNull::new(filter).ok_or(Error::CreationFailed)?;
            let filter = FilterBox::from_raw(filter);
            Ok(Self {
                inner: Rc::new(FilterRcInner {
                    filter,
                    _core: core,
                }),
            })
        }
    }

    /// Creates a weak reference to this filter.
    pub fn downgrade(&self) -> FilterWeak {
        FilterWeak {
            weak: Rc::downgrade(&self.inner),
        }
    }

    /// Returns an owned RT trigger that retains this filter.
    #[cfg(feature = "v0_3_77")]
    pub fn trigger_handle_rc(&self) -> FilterTriggerRc {
        FilterTriggerRc {
            filter: ptr::NonNull::new(self.as_raw_ptr()).expect("filter cannot be null"),
            _owner: self.clone(),
        }
    }

    /// Registers a timer on this filter's assigned data loop while retaining
    /// the filter for the complete timer lifetime.
    #[cfg(feature = "v1_2_0")]
    pub fn add_timer_from_thread_rc<F>(&self, callback: F) -> Result<FilterTimerRc, Error>
    where
        F: Fn(u64) + Send + 'static,
    {
        let data_loop = self.data_loop().ok_or(Error::CreationFailed)?;
        let source = data_loop.add_timer_from_thread(callback)?;
        // SAFETY: `FilterTimerRc` retains this filter, which retains the
        // assigned PipeWire data loop. Its source field is dropped first.
        let source = unsafe {
            std::mem::transmute::<crate::loop_::TimerSource<'_>, crate::loop_::TimerSource<'static>>(
                source,
            )
        };
        Ok(FilterTimerRc {
            source,
            _owner: self.clone(),
        })
    }

    /// Creates an owned listener builder that retains this filter.
    ///
    /// Callback data may own [`FilterPortRc`] values, allowing the registered
    /// listener to be retained in a reusable node value without self-references.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener_with_user_data<'a, D: 'a>(
        &self,
        user_data: D,
    ) -> ListenerLocalRcBuilder<'a, D> {
        let mut callbacks = ListenerLocalCallbacks::with_user_data(user_data);
        callbacks.filter = ptr::NonNull::new(self.as_raw_ptr());
        ListenerLocalRcBuilder {
            filter: self.clone(),
            callbacks,
        }
    }

    /// Creates an owned listener builder with default callback data.
    #[must_use = "Use the builder to register event callbacks"]
    pub fn add_local_listener<'a, D: Default + 'a>(&self) -> ListenerLocalRcBuilder<'a, D> {
        self.add_local_listener_with_user_data(D::default())
    }

    /// Adds an owned port whose registration retains this filter.
    pub fn add_port_with_user_data<D>(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
        user_data: D,
    ) -> Result<FilterPortRc<D>, Error> {
        let port_data = self.add_port_raw(direction, flags, properties, params)?;
        // The atomic count supports the explicitly non-Send retained-buffer
        // handoff. The registration itself remains thread-affine because it
        // contains `FilterRc`; only a narrower owner may transfer its guard.
        #[allow(clippy::arc_with_non_send_sync)]
        let registration = Arc::new(FilterPortRegistration {
            filter: self.clone(),
            port_data,
        });
        Ok(FilterPortRc {
            registration,
            user_data,
        })
    }

    /// Adds an owned port without application-specific port data.
    pub fn add_port(
        &self,
        direction: spa::utils::Direction,
        flags: FilterPortFlags,
        properties: PropertiesBox,
        params: &mut [&spa::pod::Pod],
    ) -> Result<FilterPortRc<()>, Error> {
        self.add_port_with_user_data(direction, flags, properties, params, ())
    }
}

/// Owned handle that requests processing while retaining its filter.
#[cfg(feature = "v0_3_77")]
pub struct FilterTriggerRc {
    filter: ptr::NonNull<pw_sys::pw_filter>,
    _owner: FilterRc,
}

#[cfg(feature = "v0_3_77")]
impl FilterTriggerRc {
    /// Requests one graph iteration for the retained trigger filter.
    pub fn trigger_process(&self) -> Result<(), Error> {
        let result = unsafe { pw_sys::pw_filter_trigger_process(self.filter.as_ptr()) };
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }
}

// SAFETY: the Rc owner is retained but never accessed or cloned by the data
// thread. The handle exposes only PipeWire's documented RT-safe trigger call
// and is dropped with its listener on the control thread.
#[cfg(feature = "v0_3_77")]
unsafe impl Send for FilterTriggerRc {}

/// Owned timer registered on a retained filter's assigned data loop.
#[cfg(feature = "v1_2_0")]
pub struct FilterTimerRc {
    source: crate::loop_::TimerSource<'static>,
    _owner: FilterRc,
}

#[cfg(feature = "v1_2_0")]
impl FilterTimerRc {
    /// Arms, rearms, or disables this timer.
    pub fn update_timer(&self, value: Option<Duration>, interval: Option<Duration>) -> SpaResult {
        self.source.update_timer(value, interval)
    }
}

impl Deref for FilterRc {
    type Target = Filter;

    fn deref(&self) -> &Self::Target {
        self.inner.filter.deref()
    }
}

impl AsRef<Filter> for FilterRc {
    fn as_ref(&self) -> &Filter {
        self.deref()
    }
}

/// Non-owning reference to a filter managed by [`FilterRc`].
pub struct FilterWeak {
    weak: Weak<FilterRcInner>,
}

impl FilterWeak {
    /// Attempts to upgrade this weak reference.
    pub fn upgrade(&self) -> Option<FilterRc> {
        self.weak.upgrade().map(|inner| FilterRc { inner })
    }
}

/// An owned filter-port registration that retains its shared filter.
///
/// This handle exposes only port operations. It deliberately does not expose
/// or clone the retained [`FilterRc`] from a process callback.
pub struct FilterPortRc<D = ()> {
    pub(super) registration: Arc<FilterPortRegistration>,
    user_data: D,
}

pub(crate) struct FilterPortRegistration {
    pub(crate) filter: FilterRc,
    pub(crate) port_data: ptr::NonNull<ffi::c_void>,
}

impl Drop for FilterPortRegistration {
    fn drop(&mut self) {
        unsafe {
            pw_sys::pw_filter_remove_port(self.port_data.as_ptr());
        }
    }
}

impl<D> FilterPortRc<D> {
    /// Returns the application data stored with this port handle.
    pub fn user_data(&self) -> &D {
        &self.user_data
    }

    /// Returns mutable application data stored with this port handle.
    pub fn user_data_mut(&mut self) -> &mut D {
        &mut self.user_data
    }

    /// Returns an opaque borrowed port reference suitable for identity checks.
    pub fn as_ref(&self) -> FilterPortRef<'_> {
        FilterPortRef {
            port_data: self.registration.port_data,
            filter: ptr::NonNull::new(self.registration.filter.as_raw_ptr())
                .expect("filter cannot be null"),
            lifetime: std::marker::PhantomData,
        }
    }

    /// Takes an available buffer from this port.
    pub fn dequeue_buffer(&self) -> Option<Buffer<'_>> {
        unsafe { Buffer::from_filter_raw(self.dequeue_raw_buffer(), self.registration.port_data) }
    }

    /// Takes an available buffer while retaining its owned port registration.
    ///
    /// The returned guard is intended for a bounded handoff that returns the
    /// buffer after the callback. It does not implement `Send`; a narrower
    /// application wrapper must prove any cross-thread topology.
    pub fn dequeue_retained_buffer(&self) -> Option<RetainedFilterBufferRc> {
        unsafe {
            Buffer::from_filter_rc_raw(self.dequeue_raw_buffer(), Arc::clone(&self.registration))
                .map(RetainedFilterBufferRc::from_buffer)
        }
    }

    /// Takes an available raw buffer from this port.
    ///
    /// # Safety
    ///
    /// The returned buffer, when non-null, must be queued back to this port.
    pub unsafe fn dequeue_raw_buffer(&self) -> *mut pw_sys::pw_buffer {
        pw_sys::pw_filter_dequeue_buffer(self.registration.port_data.as_ptr())
    }

    /// Queues a raw buffer back to this port.
    ///
    /// # Safety
    ///
    /// `buffer` must have been dequeued from this port and not already queued.
    pub unsafe fn queue_raw_buffer(&self, buffer: *mut pw_sys::pw_buffer) -> Result<(), Error> {
        let result = pw_sys::pw_filter_queue_buffer(self.registration.port_data.as_ptr(), buffer);
        SpaResult::from_c(result).into_result()?;
        Ok(())
    }

    /// Borrows this port for graph-independent latest-buffer I/O on one worker.
    ///
    /// The returned handle is `Send` but not `Sync`. Its mutable borrow keeps
    /// the port registration alive and prevents other safe port operations
    /// until the worker returns it.
    ///
    /// # Safety
    ///
    /// An input port must have at most one active PipeWire buffer-latest link;
    /// an output port may fan out through the implementation's fixed subscriber
    /// limit. No graph process callback or other thread may dequeue or queue
    /// buffers on this port while the handle exists. An input worker may hold
    /// only one dequeued buffer at a time. The worker must stop and return the
    /// handle before disconnecting. While the returned handle exists,
    /// PipeWireAO rejects filter disconnect, port removal, and installed-pool
    /// replacement with `EBUSY`.
    pub unsafe fn buffer_latest(&mut self) -> Result<FilterBufferLatestPort<'_>, Error> {
        let result =
            pw_sys::pw_filter_buffer_latest_worker_begin(self.registration.port_data.as_ptr());
        SpaResult::from_c(result).into_result()?;
        Ok(FilterBufferLatestPort {
            port_data: self.registration.port_data,
            idle: None,
            lifetime: std::marker::PhantomData,
            not_sync: std::marker::PhantomData,
        })
    }

    /// Gets the port's DSP buffer pointer.
    ///
    /// # Safety
    ///
    /// The caller must use the negotiated DSP format and `n_samples` to access
    /// the returned memory with the correct type and extent.
    pub unsafe fn dsp_buffer(&self, n_samples: u32) -> *mut ffi::c_void {
        pw_sys::pw_filter_get_dsp_buffer(self.registration.port_data.as_ptr(), n_samples)
    }
}

/// Caller-polled coordination of matching complete buffers from multiple inputs.
///
/// This is a client-side lease owner, not a graph scheduler. [`Self::poll`]
/// performs one bounded scan using caller-supplied monotonic time. Matching
/// leases remain borrowed through [`RendezvousBuffer`] until [`Self::finish`],
/// [`Self::cancel`], [`Self::reset`], or drop returns them.
pub struct CompleteBufferRendezvous<'p, D = ()> {
    raw: ptr::NonNull<pw_sys::pw_filter_rendezvous>,
    input_count: usize,
    required_inputs: u64,
    release_policy: RendezvousReleasePolicy,
    ports: std::marker::PhantomData<&'p mut [FilterPortRc<D>]>,
    not_sync: std::marker::PhantomData<Cell<()>>,
}

impl<'p, D> CompleteBufferRendezvous<'p, D> {
    /// Prepares a fixed input set and starts exclusive latest-buffer ownership.
    ///
    /// Preparation may allocate. Polling, buffer access, release decisions, and
    /// lease return do not allocate. The application must select this facility
    /// explicitly; PipeWireAO does not activate it from graph topology.
    ///
    /// # Safety
    ///
    /// Every port must be a latest-buffer input with at most one active link.
    /// No process callback or other thread may dequeue or queue these ports
    /// until this rendezvous is dropped. The filter must remain connected, and
    /// installed pools must remain unchanged, for the rendezvous lifetime.
    pub unsafe fn prepare(
        ports: &'p mut [FilterPortRc<D>],
        required_inputs: u64,
        release_policy: RendezvousReleasePolicy,
    ) -> io::Result<Self> {
        let raw_policy = match release_policy {
            RendezvousReleasePolicy::CompleteOrDeadline => {
                pw_sys::pw_filter_rendezvous_release_policy_PW_FILTER_RENDEZVOUS_RELEASE_COMPLETE_OR_DEADLINE
            }
            RendezvousReleasePolicy::FixedRelease => {
                pw_sys::pw_filter_rendezvous_release_policy_PW_FILTER_RENDEZVOUS_RELEASE_FIXED
            }
        };
        let raw_ports: Vec<*mut ffi::c_void> = ports
            .iter()
            .map(|port| port.registration.port_data.as_ptr())
            .collect();
        let mut raw = ptr::null_mut();
        let result = pw_sys::pw_filter_rendezvous_new(
            &mut raw,
            raw_ports.as_ptr(),
            u32::try_from(raw_ports.len()).map_err(|_| {
                io::Error::new(io::ErrorKind::InvalidInput, "too many rendezvous inputs")
            })?,
            required_inputs,
            raw_policy,
        );
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        let raw =
            ptr::NonNull::new(raw).ok_or_else(|| io::Error::from_raw_os_error(libc::EPROTO))?;
        Ok(Self {
            raw,
            input_count: ports.len(),
            required_inputs,
            release_policy,
            ports: std::marker::PhantomData,
            not_sync: std::marker::PhantomData,
        })
    }

    /// Returns the prepared input count and per-poll scan bound.
    pub const fn input_count(&self) -> usize {
        self.input_count
    }

    /// Returns the prepared nonempty required-input mask.
    pub const fn required_inputs(&self) -> u64 {
        self.required_inputs
    }

    /// Returns the prepared release behavior.
    pub const fn release_policy(&self) -> RendezvousReleasePolicy {
        self.release_policy
    }

    /// Begins one expected acquisition after prior leases were returned.
    ///
    /// `release_at_nanoseconds` is an absolute value in the caller's local
    /// Linux `CLOCK_MONOTONIC` domain. Domain replacement requires
    /// `discontinuity`; repeated or regressing identities are rejected.
    pub fn begin(
        &mut self,
        acquisition: &MetaAcquisition,
        release_at_nanoseconds: u64,
        discontinuity: bool,
    ) -> io::Result<()> {
        let result = unsafe {
            pw_sys::pw_filter_rendezvous_begin(
                self.raw.as_ptr(),
                acquisition.as_raw(),
                release_at_nanoseconds,
                discontinuity,
            )
        };
        rendezvous_operation_result(result)
    }

    /// Performs one bounded scan and returns a release decision when eligible.
    ///
    /// This method does not wait or read a clock. `monotonic_now_nanoseconds`
    /// must use the same local `CLOCK_MONOTONIC` domain as the prepared release
    /// time. A returned result retains its accepted leases until a terminal
    /// lifecycle operation returns them.
    pub fn poll(
        &mut self,
        monotonic_now_nanoseconds: u64,
    ) -> io::Result<Option<RendezvousRelease>> {
        let mut raw = std::mem::MaybeUninit::uninit();
        let result = unsafe {
            pw_sys::pw_filter_rendezvous_poll(
                self.raw.as_ptr(),
                monotonic_now_nanoseconds,
                raw.as_mut_ptr(),
            )
        };
        match result {
            0 => Ok(None),
            1 => rendezvous_release_from_raw(unsafe { raw.assume_init() }).map(Some),
            result if result < 0 => Err(io::Error::from_raw_os_error(-result)),
            _ => Err(io::Error::from_raw_os_error(libc::EPROTO)),
        }
    }

    /// Borrows one accepted complete buffer after a release decision.
    ///
    /// The buffer exposes immutable data and metadata only. Its borrow prevents
    /// a safe terminal operation from returning the underlying lease while it
    /// is still in use.
    pub fn accepted_buffer(&self, input_index: usize) -> Option<RendezvousBuffer<'_>> {
        let input_index = u32::try_from(input_index).ok()?;
        let buffer =
            unsafe { pw_sys::pw_filter_rendezvous_get_buffer(self.raw.as_ptr(), input_index) };
        ptr::NonNull::new(buffer).map(|raw| RendezvousBuffer {
            raw,
            rendezvous: std::marker::PhantomData,
        })
    }

    /// Returns every retained lease and completes the released acquisition.
    pub fn finish(&mut self) -> io::Result<()> {
        let result = unsafe { pw_sys::pw_filter_rendezvous_finish(self.raw.as_ptr()) };
        rendezvous_operation_result(result)
    }

    /// Returns every retained lease and cancels only the active acquisition.
    pub fn cancel(&mut self) -> io::Result<()> {
        let result = unsafe { pw_sys::pw_filter_rendezvous_cancel(self.raw.as_ptr()) };
        rendezvous_operation_result(result)
    }

    /// Cancels active work and clears completed-acquisition ordering state.
    pub fn reset(&mut self) -> io::Result<()> {
        let result = unsafe { pw_sys::pw_filter_rendezvous_reset(self.raw.as_ptr()) };
        rendezvous_operation_result(result)
    }

    /// Snapshots fixed single-writer classification and cleanup accounting.
    pub fn stats(&self) -> io::Result<RendezvousStats> {
        let mut raw = std::mem::MaybeUninit::uninit();
        let result =
            unsafe { pw_sys::pw_filter_rendezvous_get_stats(self.raw.as_ptr(), raw.as_mut_ptr()) };
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        let raw = unsafe { raw.assume_init() };
        Ok(RendezvousStats {
            accepted: raw.accepted,
            duplicate: raw.duplicate,
            stale: raw.stale,
            future: raw.future,
            rejected: raw.rejected,
            complete_releases: raw.complete_releases,
            deadline_releases: raw.deadline_releases,
            fixed_releases: raw.fixed_releases,
            missing_required_inputs: raw.missing_required_inputs,
            lease_returns: raw.lease_returns,
            cleanup_errors: raw.cleanup_errors,
        })
    }
}

impl<D> Drop for CompleteBufferRendezvous<'_, D> {
    fn drop(&mut self) {
        let result = unsafe { pw_sys::pw_filter_rendezvous_destroy(self.raw.as_ptr()) };
        debug_assert_eq!(result, 0, "complete-buffer rendezvous cleanup failed");
    }
}

// SAFETY: this handle borrows every port exclusively but carries only the C
// rendezvous pointer to the worker. It cannot access or drop the retained Rc
// registrations or application data from that thread. C owns the bounded
// single-writer lease state and its worker-lifetime barriers.
unsafe impl<D> Send for CompleteBufferRendezvous<'_, D> {}

/// Immutable access to one complete buffer retained by a rendezvous decision.
pub struct RendezvousBuffer<'r> {
    raw: ptr::NonNull<pw_sys::pw_buffer>,
    rendezvous: std::marker::PhantomData<&'r ()>,
}

impl RendezvousBuffer<'_> {
    fn spa_buffer(&self) -> Option<&spa_sys::spa_buffer> {
        let buffer = unsafe { self.raw.as_ref().buffer };
        unsafe { buffer.as_ref() }
    }

    /// Returns immutable mapped data descriptors for this accepted buffer.
    pub fn datas(&self) -> &[Data] {
        let Some(buffer) = self.spa_buffer() else {
            return &[];
        };
        if buffer.n_datas == 0 || buffer.datas.is_null() {
            return &[];
        }
        unsafe {
            std::slice::from_raw_parts(
                buffer.datas.cast::<Data>(),
                usize::try_from(buffer.n_datas).expect("SPA data count does not fit usize"),
            )
        }
    }

    /// Returns one immutable mapped allocation by data index.
    ///
    /// The corresponding [`Data::chunk`] identifies the valid payload range.
    /// A DMA-BUF without a mapped data pointer returns `None`.
    pub fn data(&self, index: usize) -> Option<&[u8]> {
        let buffer = self.spa_buffer()?;
        if index >= buffer.n_datas as usize || buffer.datas.is_null() {
            return None;
        }
        let data = unsafe { &*buffer.datas.add(index) };
        if data.data.is_null() {
            return None;
        }
        Some(unsafe {
            std::slice::from_raw_parts(
                data.data.cast::<u8>(),
                usize::try_from(data.maxsize).expect("SPA data size does not fit usize"),
            )
        })
    }

    /// Finds immutable metadata of type `T` attached to this accepted buffer.
    pub fn find_meta<T: Metadata>(&self) -> Option<&T> {
        let buffer = self.spa_buffer()?;
        if buffer.n_metas == 0 {
            return None;
        }
        let metadata = unsafe {
            match T::META_TYPE {
                spa_sys::SPA_META_VideoDamage => {
                    spa_sys::spa_buffer_find_meta(buffer, T::META_TYPE).cast::<T>()
                }
                _ => spa_sys::spa_buffer_find_meta_data(
                    buffer,
                    T::META_TYPE,
                    std::mem::size_of::<T>(),
                )
                .cast::<T>(),
            }
        };
        unsafe { metadata.as_ref() }
    }
}

fn rendezvous_operation_result(result: i32) -> io::Result<()> {
    if result < 0 {
        Err(io::Error::from_raw_os_error(-result))
    } else {
        Ok(())
    }
}

fn rendezvous_release_from_raw(
    raw: pw_sys::pw_filter_rendezvous_result,
) -> io::Result<RendezvousRelease> {
    let acquisition = MetaAcquisition::from_raw(raw.acquisition)
        .map_err(|_| io::Error::from_raw_os_error(libc::EPROTO))?;
    let cause = match raw.cause {
        pw_sys::pw_filter_rendezvous_release_cause_PW_FILTER_RENDEZVOUS_CAUSE_COMPLETE => {
            RendezvousReleaseCause::Complete
        }
        pw_sys::pw_filter_rendezvous_release_cause_PW_FILTER_RENDEZVOUS_CAUSE_DEADLINE => {
            RendezvousReleaseCause::Deadline
        }
        pw_sys::pw_filter_rendezvous_release_cause_PW_FILTER_RENDEZVOUS_CAUSE_FIXED => {
            RendezvousReleaseCause::FixedRelease
        }
        _ => return Err(io::Error::from_raw_os_error(libc::EPROTO)),
    };
    Ok(RendezvousRelease {
        acquisition,
        accepted_inputs: raw.accepted_inputs,
        missing_required_inputs: raw.missing_required_inputs,
        cause,
    })
}

// SAFETY: the retained Rc is never exposed or mutated from a callback. Shared
// access is limited to PipeWire's documented RT-safe port operations and
// immutable pointer identity. Listener removal precedes callback-data drop.
unsafe impl<D: Sync> Sync for FilterPortRc<D> {}

/// Exclusive worker-side access to one graph-independent latest-buffer handoff.
///
/// Obtain this handle with [`FilterPortRc::buffer_latest`]. A scoped worker is
/// the natural way to satisfy its lifetime and teardown contract.
pub struct FilterBufferLatestPort<'p> {
    port_data: ptr::NonNull<ffi::c_void>,
    idle: Option<BufferLatestIdle>,
    lifetime: std::marker::PhantomData<&'p mut ()>,
    not_sync: std::marker::PhantomData<Cell<()>>,
}

impl FilterBufferLatestPort<'_> {
    /// Takes the latest input buffer or a safely reusable output buffer.
    ///
    /// The mutable borrow prevents a worker from holding a second buffer from
    /// this port before the first buffer is returned.
    pub fn dequeue_buffer(&mut self) -> Option<Buffer<'_>> {
        let buffer = unsafe { pw_sys::pw_filter_dequeue_buffer(self.port_data.as_ptr()) };
        if buffer.is_null() {
            return None;
        }
        if let Some(idle) = &mut self.idle {
            idle.reset();
        }
        unsafe { Buffer::from_filter_raw(buffer, self.port_data) }
    }

    /// Tries to claim one input latest-buffer publication.
    ///
    /// Unlike [`Self::dequeue_buffer`], this does not inspect the ordinary
    /// PipeWire port queue or use `errno`. `Ok(None)` is the expected no-work
    /// result. Calling it on an output or ordinary port returns an error.
    pub fn try_dequeue_input(&mut self) -> io::Result<Option<Buffer<'_>>> {
        let buffer = try_dequeue_latest_raw(self.port_data)?;
        if buffer.is_null() {
            return Ok(None);
        }
        if let Some(idle) = &mut self.idle {
            idle.reset();
        }
        Ok(unsafe { Buffer::from_filter_raw(buffer, self.port_data) })
    }

    /// Returns the borrowed advisory eventfd selected for this port.
    ///
    /// The descriptor is owned by the connected filter and must not be closed.
    /// `ENODEV` means the port selected [`BufferLatestWaitPolicy::BusySpin`].
    pub fn notification_fd(&self) -> io::Result<BorrowedFd<'_>> {
        let fd = self.notification_raw_fd()?;
        // SAFETY: PipeWire owns this descriptor for at least the lifetime of
        // the connected port, which this handle borrows exclusively.
        Ok(unsafe { BorrowedFd::borrow_raw(fd) })
    }

    /// Snapshots bounded producer-side acquisition accounting.
    ///
    /// This method is valid only for an output worker. The exclusive worker
    /// must not call it concurrently with another dequeue or publication.
    pub fn stats(&self) -> io::Result<BufferLatestStats> {
        let mut raw = std::mem::MaybeUninit::<pw_sys::pw_filter_buffer_latest_stats>::uninit();
        let result = unsafe {
            pw_sys::pw_filter_get_buffer_latest_stats(self.port_data.as_ptr(), raw.as_mut_ptr())
        };
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        let raw = unsafe { raw.assume_init() };
        Ok(BufferLatestStats {
            dequeue_attempts: raw.dequeue_attempts,
            recycle_returns: raw.recycle_returns,
            buffer_probes: raw.buffer_probes,
            pool_exhaustions: raw.pool_exhaustions,
            ready_reclaims: raw.ready_reclaims,
            ready_withdrawals: raw.ready_withdrawals,
            publications: raw.publications,
            subscriber_visits: raw.subscriber_visits,
            subscriber_deliveries: raw.subscriber_deliveries,
            subscriber_supersessions: raw.subscriber_supersessions,
            subscriber_retirements: raw.subscriber_retirements,
            retired_leases: raw.retired_leases,
            zero_recipient_publications: raw.zero_recipient_publications,
            max_buffer_probes: raw.max_buffer_probes,
            max_recycle_returns: raw.max_recycle_returns,
            max_ready_withdrawals: raw.max_ready_withdrawals,
            max_subscriber_visits: raw.max_subscriber_visits,
        })
    }

    /// Waits until a buffer is available using the selected receiver policy.
    ///
    /// Eventfd is advisory: this method always treats latest-buffer shared
    /// state as authoritative and rechecks it before and after draining or
    /// waiting. EventFd and Hybrid return an error when no notification fd was
    /// negotiated; they never silently fall back to busy-spinning.
    ///
    /// This form has no deadline. The caller must arrange an in-band terminal
    /// buffer or another process-level shutdown mechanism. A worker must use
    /// one policy for its lifetime; changing it after the first wait fails.
    pub fn wait_dequeue(&mut self, policy: BufferLatestWaitPolicy) -> io::Result<Buffer<'_>> {
        match policy {
            BufferLatestWaitPolicy::BusySpin => self.wait_dequeue_busy(None).map(Option::unwrap),
            _ => self.wait_dequeue_inner(policy, None).map(Option::unwrap),
        }
    }

    /// Busy-spins until a buffer is available or `keep_running` returns false.
    ///
    /// This is the cooperative agent-loop form: each duty cycle checks the
    /// caller's running condition, tries the shared latest-buffer mailbox, and
    /// issues a processor spin hint when no work was found. It performs no
    /// clock reads, allocation, eventfd operation, or ordinary queue probe.
    /// Use [`Self::wait_dequeue`] with [`BufferLatestWaitPolicy::BusySpin`] when
    /// even the running-condition check is unnecessary.
    pub fn spin_dequeue_while<F>(&mut self, mut keep_running: F) -> io::Result<Option<Buffer<'_>>>
    where
        F: FnMut() -> bool,
    {
        if self
            .idle
            .as_ref()
            .is_some_and(|idle| idle.notification.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "latest-buffer wait policy changed after the worker started",
            ));
        }
        let port_data = self.port_data;
        let mut poller = LatestInputPoller::new(port_data)?;
        let idle = self.idle.get_or_insert_with(|| BufferLatestIdle::new(None));

        while keep_running() {
            let buffer = poller.try_dequeue()?;
            if let Some(buffer) = unsafe { Buffer::from_filter_raw(buffer, port_data) } {
                idle.idle(1);
                return Ok(Some(buffer));
            }
            idle.idle(0);
        }
        Ok(None)
    }

    /// Waits until a buffer is available or `deadline` is reached.
    ///
    /// A buffer already visible at the deadline is returned. `Ok(None)` means
    /// no buffer was visible before the bounded wait ended. BusySpin and the
    /// spinning phase of Hybrid amortize clock reads: after the first empty
    /// poll, deadline observation may be delayed by at most
    /// [`BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL`] empty polls. Eventfd
    /// waiting uses the deadline directly.
    pub fn wait_dequeue_until(
        &mut self,
        policy: BufferLatestWaitPolicy,
        deadline: Instant,
    ) -> io::Result<Option<Buffer<'_>>> {
        match policy {
            BufferLatestWaitPolicy::BusySpin => self.wait_dequeue_busy(Some(deadline)),
            _ => self.wait_dequeue_inner(policy, Some(deadline)),
        }
    }

    fn notification_raw_fd(&self) -> io::Result<RawFd> {
        let result = unsafe { pw_sys::pw_filter_get_buffer_latest_fd(self.port_data.as_ptr()) };
        if result < 0 {
            Err(io::Error::from_raw_os_error(-result))
        } else {
            Ok(result)
        }
    }

    fn wait_dequeue_busy(&mut self, deadline: Option<Instant>) -> io::Result<Option<Buffer<'_>>> {
        if self
            .idle
            .as_ref()
            .is_some_and(|idle| idle.notification.is_some())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "latest-buffer wait policy changed after the worker started",
            ));
        }
        let port_data = self.port_data;
        let mut poller = LatestInputPoller::new(port_data)?;
        let idle = self.idle.get_or_insert_with(|| BufferLatestIdle::new(None));
        let mut spin_deadline = SpinDeadline::new(deadline);

        loop {
            let buffer = poller.try_dequeue()?;
            if let Some(buffer) = unsafe { Buffer::from_filter_raw(buffer, port_data) } {
                idle.idle(1);
                return Ok(Some(buffer));
            }
            idle.idle(0);
            if spin_deadline.expired() {
                return Ok(None);
            }
        }
    }

    fn wait_dequeue_inner(
        &mut self,
        policy: BufferLatestWaitPolicy,
        deadline: Option<Instant>,
    ) -> io::Result<Option<Buffer<'_>>> {
        let notification = policy
            .notification_spin_iterations()
            .map(|spin_iterations| self.notification_raw_fd().map(|fd| (fd, spin_iterations)))
            .transpose()?;
        if self
            .idle
            .as_ref()
            .is_some_and(|idle| idle.notification != notification)
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "latest-buffer wait policy changed after the worker started",
            ));
        }
        let port_data = self.port_data;
        let idle = self
            .idle
            .get_or_insert_with(|| BufferLatestIdle::new(notification));
        let mut spin_deadline = SpinDeadline::new(deadline);

        loop {
            let buffer = try_dequeue_latest_raw(port_data)?;
            if let Some(buffer) = unsafe { Buffer::from_filter_raw(buffer, port_data) } {
                idle.idle(1);
                return Ok(Some(buffer));
            }

            let fd = match idle.idle(0) {
                BufferLatestIdleAction::Wait(fd) => fd,
                BufferLatestIdleAction::Worked | BufferLatestIdleAction::Spin => {
                    if spin_deadline.expired() {
                        return Ok(None);
                    }
                    continue;
                }
            };

            drain_eventfd(fd)?;

            // Close the check/drain race before sleeping. A publication either
            // appears here or leaves the eventfd readable for poll below.
            let buffer = try_dequeue_latest_raw(port_data)?;
            if let Some(buffer) = unsafe { Buffer::from_filter_raw(buffer, port_data) } {
                idle.idle(1);
                return Ok(Some(buffer));
            }
            if !poll_eventfd(fd, deadline)? {
                return Ok(None);
            }
        }
    }
}

impl Drop for FilterBufferLatestPort<'_> {
    fn drop(&mut self) {
        let result = unsafe { pw_sys::pw_filter_buffer_latest_worker_end(self.port_data.as_ptr()) };
        debug_assert_eq!(result, 0, "latest-buffer worker ownership ended twice");
    }
}

fn try_dequeue_latest_raw(
    port_data: ptr::NonNull<ffi::c_void>,
) -> io::Result<*mut pw_sys::pw_buffer> {
    let mut buffer = ptr::null_mut();
    let result =
        unsafe { pw_sys::pw_filter_try_dequeue_buffer_latest(port_data.as_ptr(), &mut buffer) };
    latest_dequeue_result(result, buffer)
}

fn latest_dequeue_result(
    result: i32,
    buffer: *mut pw_sys::pw_buffer,
) -> io::Result<*mut pw_sys::pw_buffer> {
    match result {
        0 => Ok(ptr::null_mut()),
        1 if !buffer.is_null() => Ok(buffer),
        1 => Err(io::Error::from_raw_os_error(libc::EPROTO)),
        result if result < 0 => Err(io::Error::from_raw_os_error(-result)),
        _ => Err(io::Error::from_raw_os_error(libc::EPROTO)),
    }
}

/// Owns the C live-link pin for one continuous busy-spin polling interval.
///
/// The C poller keeps the pin after an empty dequeue and releases it on link
/// change, success, or error. `Drop` covers cancellation, deadline, and unwind
/// exits so a synchronous live detach cannot be left waiting on this worker.
struct LatestInputPoller {
    raw: pw_sys::pw_filter_buffer_latest_poller,
}

impl LatestInputPoller {
    fn new(port_data: ptr::NonNull<ffi::c_void>) -> io::Result<Self> {
        let mut raw = std::mem::MaybeUninit::uninit();
        let result = unsafe {
            pw_sys::pw_filter_buffer_latest_poller_init(raw.as_mut_ptr(), port_data.as_ptr())
        };
        if result < 0 {
            return Err(io::Error::from_raw_os_error(-result));
        }
        Ok(Self {
            raw: unsafe { raw.assume_init() },
        })
    }

    fn try_dequeue(&mut self) -> io::Result<*mut pw_sys::pw_buffer> {
        let mut buffer = ptr::null_mut();
        let result = unsafe {
            pw_sys::pw_filter_buffer_latest_poller_try_dequeue(&mut self.raw, &mut buffer)
        };
        latest_dequeue_result(result, buffer)
    }
}

impl Drop for LatestInputPoller {
    fn drop(&mut self) {
        unsafe { pw_sys::pw_filter_buffer_latest_poller_clear(&mut self.raw) };
    }
}

struct SpinDeadline {
    deadline: Option<Instant>,
    polls_until_check: u32,
}

impl SpinDeadline {
    const fn new(deadline: Option<Instant>) -> Self {
        Self {
            deadline,
            polls_until_check: 0,
        }
    }

    fn expired(&mut self) -> bool {
        let Some(deadline) = self.deadline else {
            return false;
        };
        if self.polls_until_check > 0 {
            self.polls_until_check -= 1;
            return false;
        }
        self.polls_until_check = BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL - 1;
        Instant::now() >= deadline
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum BufferLatestIdleAction {
    Worked,
    Spin,
    Wait(RawFd),
}

/// Agrona-style work-count idle state for one latest-buffer agent.
///
/// A positive work count resets Hybrid to its prepared spin phase. Advisory
/// wakeups do not reset it; only authoritative shared-state work does.
struct BufferLatestIdle {
    notification: Option<(RawFd, u32)>,
    idle_cycles: u32,
}

impl BufferLatestIdle {
    const fn new(notification: Option<(RawFd, u32)>) -> Self {
        Self {
            notification,
            idle_cycles: 0,
        }
    }

    fn idle(&mut self, work_count: usize) -> BufferLatestIdleAction {
        if work_count > 0 {
            self.reset();
            return BufferLatestIdleAction::Worked;
        }

        let Some((fd, max_spins)) = self.notification else {
            std::hint::spin_loop();
            return BufferLatestIdleAction::Spin;
        };
        if self.idle_cycles < max_spins {
            self.idle_cycles += 1;
            std::hint::spin_loop();
            BufferLatestIdleAction::Spin
        } else {
            BufferLatestIdleAction::Wait(fd)
        }
    }

    const fn reset(&mut self) {
        self.idle_cycles = 0;
    }
}

fn drain_eventfd(fd: RawFd) -> io::Result<()> {
    let mut count = 0u64;
    loop {
        let result = unsafe {
            libc::read(
                fd,
                ptr::addr_of_mut!(count).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        if result == std::mem::size_of::<u64>() as isize {
            return Ok(());
        }
        if result >= 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "short read from latest-buffer eventfd",
            ));
        }
        let error = io::Error::last_os_error();
        match error.kind() {
            io::ErrorKind::Interrupted => continue,
            io::ErrorKind::WouldBlock => return Ok(()),
            _ => return Err(error),
        }
    }
}

fn poll_eventfd(fd: RawFd, deadline: Option<Instant>) -> io::Result<bool> {
    loop {
        let timeout = match deadline {
            None => -1,
            Some(deadline) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    return Ok(false);
                }
                poll_timeout_millis(remaining)
            }
        };
        let mut descriptor = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe { libc::poll(ptr::addr_of_mut!(descriptor), 1, timeout) };
        if result > 0 {
            if descriptor.revents & libc::POLLNVAL != 0 {
                return Err(io::Error::from_raw_os_error(libc::EBADF));
            }
            if descriptor.revents & (libc::POLLERR | libc::POLLHUP) != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "latest-buffer eventfd closed",
                ));
            }
            return Ok(true);
        }
        if result == 0 {
            return Ok(false);
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

fn poll_timeout_millis(duration: Duration) -> libc::c_int {
    duration
        .as_millis()
        .saturating_add(u128::from(duration.subsec_nanos() % 1_000_000 != 0))
        .min(libc::c_int::MAX as u128) as libc::c_int
}

// SAFETY: construction requires an exclusive mutable borrow of the port and
// an explicit promise that one latest-mode worker owns all buffer operations.
// The lifetime prevents the port registration from being removed first.
unsafe impl Send for FilterBufferLatestPort<'_> {}

/// Builder for local callbacks that retains a shared filter.
pub struct ListenerLocalRcBuilder<'a, D> {
    filter: FilterRc,
    callbacks: ListenerLocalCallbacks<'a, D>,
}

impl<'a, D> ListenerLocalRcBuilder<'a, D> {
    #[must_use = "Call `.register()` to start receiving events"]
    pub fn state_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterState, FilterState) + Send + 'a,
    {
        *self.callbacks.state_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn io_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, *mut ffi::c_void, u32) + Send + 'a,
    {
        *self.callbacks.io_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn param_changed<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<FilterPortRef<'_>>, u32, Option<&spa::pod::Pod>) + Send + 'a,
    {
        *self.callbacks.param_changed.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn add_buffer<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterPortRef<'_>, *mut pw_sys::pw_buffer) + Send + 'a,
    {
        *self.callbacks.add_buffer.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn remove_buffer<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, FilterPortRef<'_>, *mut pw_sys::pw_buffer) + Send + 'a,
    {
        *self.callbacks.remove_buffer.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn process<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, Option<&spa_sys::spa_io_position>) + Send + 'a,
    {
        *self.callbacks.process.get_mut() = Some(Box::new(callback));
        self
    }

    #[must_use = "Call `.register()` to start receiving events"]
    pub fn drained<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D) + Send + 'a,
    {
        *self.callbacks.drained.get_mut() = Some(Box::new(callback));
        self
    }

    #[cfg(feature = "v0_3_39")]
    #[must_use = "Call `.register()` to start receiving events"]
    pub fn command<F>(mut self, callback: F) -> Self
    where
        F: FnMut(&Filter, &D, *const spa_sys::spa_command) + Send + 'a,
    {
        *self.callbacks.command.get_mut() = Some(Box::new(callback));
        self
    }

    /// Registers the selected callbacks and retains their filter.
    pub fn register(self) -> Result<FilterListenerRc<'a, D>, Error>
    where
        D: Sync,
    {
        let (events, data) = self.callbacks.into_raw();
        let (listener, data) = unsafe {
            let listener: Box<spa_sys::spa_hook> = Box::new(std::mem::zeroed());
            let raw_listener = Box::into_raw(listener);
            let raw_data = Box::into_raw(data);
            pw_sys::pw_filter_add_listener(
                self.filter.as_raw_ptr(),
                raw_listener,
                events.as_ref().get_ref(),
                raw_data.cast(),
            );
            (Box::from_raw(raw_listener), Box::from_raw(raw_data))
        };
        Ok(FilterListenerRc {
            listener,
            _events: events,
            _data: data,
            _filter: self.filter,
        })
    }
}

/// Owned filter callbacks that retain their shared filter.
#[must_use = "Keep the listener alive in order to receive events"]
pub struct FilterListenerRc<'a, D> {
    listener: Box<spa_sys::spa_hook>,
    _events: Pin<Box<pw_sys::pw_filter_events>>,
    _data: Box<ListenerLocalCallbacks<'a, D>>,
    _filter: FilterRc,
}

impl<D> FilterListenerRc<'_, D> {
    /// Stops receiving events by consuming this listener.
    pub fn unregister(self) {}
}

impl<D> Drop for FilterListenerRc<'_, D> {
    fn drop(&mut self) {
        spa::utils::hook::remove(*self.listener);
    }
}

#[cfg(test)]
mod latest_wait_tests {
    use super::*;
    use spa::buffer::meta::AcquisitionIdentity;

    fn acquisition(sequence: u64) -> MetaAcquisition {
        let mut domain = [0; spa::buffer::meta::ACQUISITION_DOMAIN_SIZE];
        domain[0] = 1;
        let domain = spa::buffer::meta::AcquisitionDomain::new(domain).unwrap();
        let mut acquisition = MetaAcquisition::new();
        acquisition
            .set_identity(AcquisitionIdentity::new(domain, 2, sequence))
            .unwrap();
        acquisition
    }

    #[test]
    fn policy_values_match_daemon_contract() {
        assert_eq!(BUFFER_LATEST_LINK_IO_ID, 13);
        assert_eq!(
            MAX_RENDEZVOUS_INPUTS,
            pw_sys::PW_FILTER_RENDEZVOUS_MAX_INPUTS as usize
        );
        assert_eq!(
            BufferLatestWaitPolicy::BusySpin.port_property_value(),
            "busy-spin"
        );
        assert_eq!(
            BufferLatestWaitPolicy::EventFd.port_property_value(),
            "eventfd"
        );
        assert_eq!(
            BufferLatestWaitPolicy::Hybrid {
                spin_iterations: 64
            }
            .port_property_value(),
            "hybrid"
        );
    }

    #[test]
    fn rendezvous_release_decodes_native_result() {
        let acquisition = acquisition(42);
        let raw = pw_sys::pw_filter_rendezvous_result {
            acquisition: *acquisition.as_raw(),
            accepted_inputs: 0b101,
            missing_required_inputs: 0b010,
            cause: pw_sys::pw_filter_rendezvous_release_cause_PW_FILTER_RENDEZVOUS_CAUSE_DEADLINE,
            reserved: 0,
        };

        let release = rendezvous_release_from_raw(raw).unwrap();
        assert_eq!(
            release.acquisition.identity().unwrap().unwrap().sequence(),
            42
        );
        assert_eq!(release.accepted_inputs, 0b101);
        assert_eq!(release.missing_required_inputs, 0b010);
        assert_eq!(release.cause, RendezvousReleaseCause::Deadline);
    }

    #[test]
    fn rendezvous_rejects_unknown_native_release_cause() {
        let acquisition = acquisition(42);
        let raw = pw_sys::pw_filter_rendezvous_result {
            acquisition: *acquisition.as_raw(),
            accepted_inputs: 0,
            missing_required_inputs: 0,
            cause: u32::MAX,
            reserved: 0,
        };

        assert_eq!(
            rendezvous_release_from_raw(raw).unwrap_err().raw_os_error(),
            Some(libc::EPROTO)
        );
    }

    #[test]
    fn rendezvous_worker_handle_is_send_without_sending_port_data() {
        fn require_send<T: Send>() {}

        require_send::<CompleteBufferRendezvous<'static, Rc<()>>>();
    }

    #[test]
    fn latest_link_callback_is_snapshotted_without_exposing_shared_io() {
        let active = spa_sys::spa_io_buffers_latest_link {
            id: 41,
            flags: 1,
            io: ptr::NonNull::<spa_sys::spa_io_buffers_latest>::dangling().as_ptr(),
            notify_fd: 17,
            reserved: 0,
        };
        let info = unsafe {
            BufferLatestLinkInfo::from_io_changed(
                BUFFER_LATEST_LINK_IO_ID,
                ptr::addr_of!(active).cast(),
                size_of_val(&active) as u32,
            )
        }
        .expect("valid latest link must decode");
        assert_eq!(
            info,
            BufferLatestLinkInfo {
                id: 41,
                active: true,
                notification_fd: Some(17),
            }
        );

        let retired = spa_sys::spa_io_buffers_latest_link {
            flags: 0,
            notify_fd: -1,
            ..active
        };
        let info = unsafe {
            BufferLatestLinkInfo::from_io_changed(
                BUFFER_LATEST_LINK_IO_ID,
                ptr::addr_of!(retired).cast(),
                size_of_val(&retired) as u32,
            )
        }
        .expect("valid retirement must decode");
        assert!(!info.active);
        assert_eq!(info.notification_fd, None);
    }

    #[test]
    fn hybrid_idle_resets_only_after_work() {
        let mut idle = BufferLatestIdle::new(Some((7, 2)));

        assert_eq!(idle.idle(0), BufferLatestIdleAction::Spin);
        assert_eq!(idle.idle(0), BufferLatestIdleAction::Spin);
        assert_eq!(idle.idle(0), BufferLatestIdleAction::Wait(7));
        assert_eq!(idle.idle(0), BufferLatestIdleAction::Wait(7));
        assert_eq!(idle.idle(1), BufferLatestIdleAction::Worked);
        assert_eq!(idle.idle(0), BufferLatestIdleAction::Spin);
    }

    #[test]
    fn busy_spin_and_eventfd_idle_without_hidden_phases() {
        let mut busy_spin = BufferLatestIdle::new(None);
        let mut eventfd = BufferLatestIdle::new(Some((11, 0)));

        assert_eq!(busy_spin.idle(0), BufferLatestIdleAction::Spin);
        assert_eq!(eventfd.idle(0), BufferLatestIdleAction::Wait(11));
    }

    #[test]
    fn spin_deadline_check_is_iteration_bounded() {
        let deadline = Instant::now() + Duration::from_secs(3_600);
        let mut check = SpinDeadline::new(Some(deadline));

        assert!(!check.expired());
        assert_eq!(
            check.polls_until_check,
            BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL - 1
        );
        for remaining in (1..BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL).rev() {
            assert!(!check.expired());
            assert_eq!(check.polls_until_check, remaining - 1);
        }
        assert!(!check.expired());
        assert_eq!(
            check.polls_until_check,
            BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL - 1
        );
    }

    #[test]
    fn expired_spin_deadline_is_observed_on_first_empty_poll() {
        let mut check = SpinDeadline::new(Some(Instant::now()));

        assert!(check.expired());
    }

    #[test]
    fn absent_spin_deadline_never_checks_or_expires() {
        let mut check = SpinDeadline::new(None);

        for _ in 0..BUFFER_LATEST_SPIN_DEADLINE_CHECK_INTERVAL * 2 {
            assert!(!check.expired());
        }
        assert_eq!(check.polls_until_check, 0);
    }

    #[test]
    fn poll_timeout_rounds_up_and_saturates() {
        assert_eq!(poll_timeout_millis(Duration::from_nanos(1)), 1);
        assert_eq!(poll_timeout_millis(Duration::from_millis(1)), 1);
        assert_eq!(
            poll_timeout_millis(Duration::from_millis(1) + Duration::from_nanos(1)),
            2
        );
        assert_eq!(poll_timeout_millis(Duration::MAX), libc::c_int::MAX);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn eventfd_poll_and_drain_are_advisory() {
        use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};

        let fd = unsafe { libc::eventfd(0, libc::EFD_CLOEXEC | libc::EFD_NONBLOCK) };
        assert!(fd >= 0);
        let fd = unsafe { OwnedFd::from_raw_fd(fd) };
        let count = 3u64;
        let written = unsafe {
            libc::write(
                fd.as_raw_fd(),
                ptr::addr_of!(count).cast(),
                std::mem::size_of::<u64>(),
            )
        };
        assert_eq!(written, std::mem::size_of::<u64>() as isize);

        assert!(poll_eventfd(
            fd.as_raw_fd(),
            Some(Instant::now() + Duration::from_secs(1))
        )
        .expect("eventfd poll failed"));
        drain_eventfd(fd.as_raw_fd()).expect("eventfd drain failed");
        drain_eventfd(fd.as_raw_fd()).expect("stale eventfd drain must be harmless");
        assert!(!poll_eventfd(fd.as_raw_fd(), Some(Instant::now()))
            .expect("expired eventfd poll failed"));
    }
}
