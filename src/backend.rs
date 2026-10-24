use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Display;
use std::mem::take;
use std::sync::{Arc, Mutex};
use std::thread::{spawn, JoinHandle};
use std::time::{Duration, Instant};

use futures_lite::future::yield_now;
use futures_lite::FutureExt;
use log::{debug, trace, warn};
use loole::{unbounded, Receiver, Sender};
use windows::Win32::Foundation::ERROR_GRAPHICS_DDCCI_VCP_NOT_SUPPORTED;

use crate::capabilities::vcp_values;
use crate::config::Config;
use crate::monitors::{Monitor, MonitorConnection, MonitorPath};
use crate::runtime::reducing_spsc::{Reducible, ReducingReceiver, ReducingSender, TryRecvError};
use crate::runtime::{block_on, reducing_spsc, LocalExecutor, Timer};
use crate::utils::error::TracedError;
use crate::utils::extensions::{ChannelExt, MutexExt};
use crate::CustomEvent;

#[derive(Debug, Clone)]
enum BackendCommand {
    Stop,
    RefreshMonitors,
    RefreshMonitorsIn(Duration),
    QueryBrightness(Option<Duration>),
    SetBrightness(MonitorPath, u32),
    Feature(MonitorPath, Feature, FeatureCommand),
    QueryFeatures(Vec<Feature>)
}

pub struct MonitorController {
    sender: Sender<BackendCommand>,
    thread: Option<JoinHandle<()>>
}

impl MonitorController {
    pub fn new(main_sender: Sender<CustomEvent>, config: Arc<Mutex<Config>>) -> Self {
        let (sender, receiver) = unbounded();
        sender.send_ignore(BackendCommand::RefreshMonitors);
        let thread = Some(spawn(move || block_on(Self::worker_task(main_sender, receiver, config))));
        Self { sender, thread }
    }

    async fn worker_task(sender: Sender<CustomEvent>, receiver: Receiver<BackendCommand>, config: Arc<Mutex<Config>>) {
        let executor = LocalExecutor::default();
        let mut monitor_map: BTreeMap<MonitorPath, ReducingSender<MonitorCommand>> = BTreeMap::new();
        let mut delayed_query: Option<Instant> = None;
        let mut delayed_refresh: Option<Instant> = None;

        executor
            .run(async {
                loop {
                    let query_event = async {
                        delayed_query
                            .map(Timer::at)
                            .unwrap_or_else(Timer::never)
                            .await;
                        BackendCommand::QueryBrightness(None)
                    };
                    let refresh_event = async {
                        delayed_refresh
                            .map(Timer::at)
                            .unwrap_or_else(Timer::never)
                            .await;
                        BackendCommand::RefreshMonitors
                    };
                    let command_event = async { receiver.recv_async().await.expect("Controller disappeared") };
                    match query_event.or(refresh_event).or(command_event).await {
                        BackendCommand::QueryBrightness(None) => {
                            delayed_query.take();
                            let config = config.lock_no_poison();
                            monitor_map.iter().for_each(|(p, s)| {
                                s.send_ignore(MonitorCommand::brightness(BrightnessCommand::Query {
                                    target: config
                                        .restore_from_config
                                        .then(|| config.monitors.get(p))
                                        .flatten()
                                        .and_then(|s| s.saved_brightness)
                                }))
                            });
                        }
                        BackendCommand::RefreshMonitors => {
                            delayed_refresh.take();
                            trace!("Refreshing monitor list");
                            let mut current_monitors = Monitor::find_all()
                                .map_err(|err| log::warn!("Failed to enumerate monitors: {err}"))
                                .unwrap_or_default();
                            debug!("Skipping over unnamed monitors as they are likely integrated displays");
                            current_monitors.retain(|m| !m.name().is_empty());

                            let old_monitors = take(&mut monitor_map).into_keys().collect::<BTreeSet<_>>();

                            // Yield to allow the monitor tasks to finish
                            yield_now().await;
                            executor.clean_pending_tasks();

                            for path in &old_monitors {
                                if current_monitors.iter().all(|m| m.path() != path) {
                                    sender.send_ignore(CustomEvent::MonitorRemoved { path: path.clone() });
                                }
                            }

                            for monitor in current_monitors {
                                let path = monitor.path().clone();
                                if !old_monitors.contains(&path) {
                                    sender.send_ignore(CustomEvent::MonitorAdded {
                                        path: path.clone(),
                                        name: monitor.name().to_string()
                                    });
                                }
                                let (tx, rx) = reducing_spsc::channel();
                                let shown = config.lock_no_poison().shown_features();
                                if !old_monitors.contains(&path) && !shown.is_empty() {
                                    // Queued behind the brightness restore, which matters more at startup
                                    tx.send_ignore(MonitorCommand::query_features(&shown));
                                }
                                executor.spawn(monitor_task(monitor, sender.clone(), rx));
                                monitor_map.insert(path, tx);
                            }

                            delayed_query = Some(Instant::now());
                        }
                        BackendCommand::RefreshMonitorsIn(delay) => {
                            // Each new event pushes the refresh back, so a burst of events
                            // (Windows sends several while waking up) becomes one refresh
                            let at = Instant::now() + delay;
                            delayed_refresh = Some(delayed_refresh.map_or(at, |current| current.max(at)));
                        }
                        BackendCommand::QueryBrightness(Some(delay)) => {
                            delayed_query = delayed_query
                                .into_iter()
                                .chain([Instant::now() + delay])
                                .min();
                        }
                        BackendCommand::SetBrightness(p, v) => {
                            monitor_map
                                .get(&p)
                                .map(|s| s.send_ignore(MonitorCommand::brightness(BrightnessCommand::Set { value: v, notify: false })))
                                .unwrap_or_else(|| warn!("Unknown monitor {:?}", p));
                            let mut config = config.lock_no_poison();
                            if config.restore_from_config {
                                config.monitors.entry(p).or_default().saved_brightness = Some(v);
                                config.dirty = true;
                            }
                        }
                        BackendCommand::QueryFeatures(features) => {
                            monitor_map
                                .values()
                                .for_each(|s| s.send_ignore(MonitorCommand::query_features(&features)));
                        }
                        BackendCommand::Feature(p, feature, command) => {
                            monitor_map
                                .get(&p)
                                .map(|s| s.send_ignore(MonitorCommand::feature(feature, command)))
                                .unwrap_or_else(|| warn!("Unknown monitor {:?}", p));
                        }
                        BackendCommand::Stop => break
                    }
                }
            })
            .await;
    }

    pub fn refresh_brightness(&self) {
        self.send_command(BackendCommand::QueryBrightness(None));
    }

    pub fn shutdown(&self) {
        self.send_command(BackendCommand::Stop);
    }

    fn send_command(&self, command: BackendCommand) {
        self.sender.send_ignore(command)
    }

    pub fn create_proxy(&self) -> MonitorControllerProxy {
        MonitorControllerProxy(self.sender.clone())
    }
}

impl Drop for MonitorController {
    fn drop(&mut self) {
        let _ = self.sender.send(BackendCommand::Stop);
        if let Some(handle) = self.thread.take() {
            debug!("Waiting for worker thread to shutdown!");
            handle.join().expect("worker thread panic");
        }
    }
}

/// Monitor settings beyond brightness, read and written as raw VCP codes.
#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub enum Feature {
    Volume,
    Input,
    Power
}

impl Feature {
    pub const ALL: [Feature; 3] = [Feature::Volume, Feature::Input, Feature::Power];

    pub fn vcp_code(self) -> u8 {
        match self {
            Feature::Volume => 0x62,
            Feature::Input => 0x60,
            Feature::Power => 0xD6
        }
    }
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
pub struct FeatureValue {
    pub current: u32,
    pub max: u32
}

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum FeatureCommand {
    Query,
    Set(u32)
}

impl Reducible for FeatureCommand {
    fn reduce(self, other: Self) -> Self {
        match (self, other) {
            // A pending write reports its result anyway, so a query on top adds nothing
            (FeatureCommand::Set(value), FeatureCommand::Query) => FeatureCommand::Set(value),
            (_, next) => next
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum BrightnessCommand {
    Query { target: Option<u32> },
    Set { value: u32, notify: bool }
}

impl Reducible for BrightnessCommand {
    fn reduce(self, other: Self) -> Self {
        use BrightnessCommand::*;
        match (self, other) {
            (Query { target: current }, Query { target: next }) => Query { target: next.or(current) },
            (Query { .. }, Set { value, .. }) => Query { target: Some(value) },
            (Set { value, .. }, Query { target }) => Query {
                target: Some(target.unwrap_or(value))
            },
            (Set { .. }, Set { value, notify }) => Set { value, notify }
        }
    }
}

/// Everything still pending for one monitor. New commands merge into it, so a burst of
/// slider moves becomes one write without dropping unrelated work.
#[derive(Debug, Copy, Clone, Default)]
struct MonitorCommand {
    brightness: Option<BrightnessCommand>,
    features: [Option<FeatureCommand>; Feature::ALL.len()]
}

impl MonitorCommand {
    fn brightness(command: BrightnessCommand) -> Self {
        Self {
            brightness: Some(command),
            ..Default::default()
        }
    }

    fn feature(feature: Feature, command: FeatureCommand) -> Self {
        let mut result = Self::default();
        result.features[feature as usize] = Some(command);
        result
    }

    fn query_features(features: &[Feature]) -> Self {
        let mut result = Self::default();
        for &feature in features {
            result.features[feature as usize] = Some(FeatureCommand::Query);
        }
        result
    }

    fn is_empty(&self) -> bool {
        self.brightness.is_none() && self.features.iter().all(Option::is_none)
    }
}

fn merge<T: Reducible>(current: Option<T>, next: Option<T>) -> Option<T> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.reduce(next)),
        (current, next) => next.or(current)
    }
}

impl Reducible for MonitorCommand {
    fn reduce(self, other: Self) -> Self {
        let mut features = self.features;
        for (current, next) in features.iter_mut().zip(other.features) {
            *current = merge(*current, next);
        }
        Self {
            brightness: merge(self.brightness, other.brightness),
            features
        }
    }
}

async fn monitor_task(monitor: Monitor, sender: Sender<CustomEvent>, mut control: ReducingReceiver<MonitorCommand>) {
    let mut current_brightness = None;
    let mut feature_values: [Option<FeatureValue>; Feature::ALL.len()] = Default::default();
    let mut inputs_read = false;
    let mut cached_connection: Option<MonitorConnection> = None;

    let mut next_command = None;
    let mut dirty = false;

    loop {
        let mut command = match next_command.take().ok_or(()).or(control.try_recv()) {
            Err(TryRecvError::Closed) => break,
            Err(TryRecvError::Empty) => {
                if dirty {
                    debug!("Attempting to write save new settings for {}", monitor.name());
                    if let Some(connection) = &cached_connection {
                        retry(|| connection.save_settings())
                            .await
                            .unwrap_or_else(|err| warn!("Failed to save new settings for {}: {}", monitor.name(), err));
                    }
                    dirty = false;
                }
                trace!("Closing connection to {} and going to sleep", monitor.name());
                cached_connection = None;
                next_command = control.recv().await;
                continue;
            }
            Ok(command) => command
        };
        if let Some(BrightnessCommand::Set { value, .. }) = command.brightness {
            if Some(value) == current_brightness {
                trace!("Brightness already at requested level {}", value);
                command.brightness = None;
            }
        }
        if command.is_empty() {
            continue;
        }

        if cached_connection.is_none() {
            trace!("Opening connection to {}", monitor.name());
            cached_connection = retry(|| monitor.open())
                .await
                .map_err(|err| warn!("Failed to connect to monitor: {err}"))
                .ok();
        }
        let connection = match cached_connection.as_ref() {
            Some(conn) => conn,
            None => continue
        };
        match command.brightness {
            None => {}
            Some(BrightnessCommand::Query { target }) => {
                trace!("Attempting to read brightness of {}", monitor.name());
                match retry(|| connection.get_brightness()).await {
                    Ok((brightness, range)) => {
                        if range != (0..=100) {
                            warn!("unexpected brightness range: {:?}", range);
                        }
                        current_brightness = Some(brightness);
                        match target {
                            Some(target) if target != brightness => {
                                log::info!(
                                    "Restoring saved brightness for {} (current: {}, saved: {})",
                                    monitor.name(),
                                    brightness,
                                    target
                                );
                                next_command = Some(MonitorCommand::brightness(BrightnessCommand::Set { value: target, notify: true }));
                            }
                            _ => {
                                sender.send_ignore(CustomEvent::BrightnessChanged {
                                    path: monitor.path().clone(),
                                    value: brightness
                                });
                            }
                        }
                    }
                    Err(err) => warn!("Failed to query brightness: {err}")
                }
            }
            Some(BrightnessCommand::Set { value, notify }) => {
                trace!("Attempting to set brightness of {}", monitor.name());
                let success = retry(|| connection.set_brightness(value))
                    .await
                    .map_err(|err| warn!("Failed to set brightness: {err}"))
                    .is_ok();
                if success {
                    current_brightness = Some(value);
                    dirty = true;
                }
                if notify {
                    if let Some(current) = current_brightness {
                        sender.send_ignore(CustomEvent::BrightnessChanged {
                            path: monitor.path().clone(),
                            value: current
                        });
                    }
                }
            }
        }
        if command.features[Feature::Input as usize] == Some(FeatureCommand::Query) && !inputs_read {
            // The input list only comes from the capabilities string, which is slow to read, so once is enough
            inputs_read = true;
            match retry(|| connection.get_capabilities()).await {
                Ok(caps) => {
                    let mut inputs = vcp_values(&caps, Feature::Input.vcp_code()).unwrap_or_default();
                    // Some monitors list 0, which isn't an input
                    inputs.retain(|&v| v != 0);
                    if !inputs.is_empty() {
                        sender.send_ignore(CustomEvent::InputsFound {
                            path: monitor.path().clone(),
                            inputs
                        });
                    }
                }
                Err(err) => debug!("No capabilities from {}: {err}", monitor.name())
            }
        }
        for (feature, pending) in Feature::ALL.into_iter().zip(command.features) {
            let known = &mut feature_values[feature as usize];
            match pending {
                None => continue,
                Some(FeatureCommand::Query) => {
                    trace!("Attempting to read {:?} of {}", feature, monitor.name());
                    *known = retry_while(|| connection.get_vcp(feature.vcp_code()), |err| !is_unsupported(err))
                        .await
                        .map(|(current, max)| FeatureValue { current, max })
                        .map_err(|err| debug!("{:?} unavailable on {}: {err}", feature, monitor.name()))
                        .ok();
                }
                Some(FeatureCommand::Set(value)) if known.map(|v| v.current) == Some(value) => {
                    trace!("{:?} of {} already at {}", feature, monitor.name(), value);
                    continue;
                }
                Some(FeatureCommand::Set(value)) => {
                    trace!("Attempting to set {:?} of {} to {}", feature, monitor.name(), value);
                    // No read-back: after an input switch or power off the monitor often stops answering
                    match retry(|| connection.set_vcp(feature.vcp_code(), value)).await {
                        Ok(()) => {
                            *known = Some(FeatureValue {
                                current: value,
                                max: known.map_or(value, |v| v.max)
                            });
                            // Only volume is a setting worth persisting in the monitor itself
                            dirty |= feature == Feature::Volume;
                        }
                        Err(err) => warn!("Failed to set {:?} of {}: {err}", feature, monitor.name())
                    }
                }
            }
            sender.send_ignore(CustomEvent::FeatureChanged {
                path: monitor.path().clone(),
                feature,
                value: *known
            });
        }
        Timer::after(Duration::from_millis(250)).await;
    }

    trace!("Monitor task for {} is shutting down", monitor.name());
}

fn is_unsupported(err: &TracedError) -> bool {
    err.code() == Some(ERROR_GRAPHICS_DDCCI_VCP_NOT_SUPPORTED)
}

async fn retry<R, E: Display, F: FnMut() -> std::result::Result<R, E>>(op: F) -> std::result::Result<R, E> {
    retry_while(op, |_| true).await
}

async fn retry_while<R, E: Display, F, P>(mut op: F, should_retry: P) -> std::result::Result<R, E>
where
    F: FnMut() -> std::result::Result<R, E>,
    P: Fn(&E) -> bool
{
    let mut tries = 0;
    let mut backoff_ms = 100;
    loop {
        match op() {
            Ok(result) => return Ok(result),
            Err(err) if tries <= 4 && should_retry(&err) => {
                debug!("Retrying in {}: {}", backoff_ms, err);
                Timer::after(Duration::from_millis(backoff_ms)).await;
                tries += 1;
                backoff_ms *= 2;
            }
            Err(err) => return Err(err)
        }
    }
}

#[derive(Clone)]
pub struct MonitorControllerProxy(Sender<BackendCommand>);

impl MonitorControllerProxy {
    pub fn set_brightness(&self, monitor: MonitorPath, value: u32) {
        self.send_command(BackendCommand::SetBrightness(monitor, value));
    }

    /// Reads these settings from every monitor, e.g. once they're turned on in the settings.
    pub fn query_features(&self, features: Vec<Feature>) {
        self.send_command(BackendCommand::QueryFeatures(features));
    }

    pub fn set_feature(&self, monitor: MonitorPath, feature: Feature, value: u32) {
        self.send_command(BackendCommand::Feature(monitor, feature, FeatureCommand::Set(value)));
    }

    pub fn refresh_brightness_in(&self, delay: Duration) {
        self.send_command(BackendCommand::QueryBrightness(Some(delay)));
    }

    pub fn refresh_monitors_in(&self, delay: Duration) {
        self.send_command(BackendCommand::RefreshMonitorsIn(delay));
    }

    fn send_command(&self, command: BackendCommand) {
        self.0.send_ignore(command);
    }
}
