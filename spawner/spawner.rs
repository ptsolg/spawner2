use crate::limit_checker::{EnabledOsLimits, LimitChecker};
use crate::pipe::{ReadPipe, WritePipe};
use crate::process::{
    ExitStatus, Group, GroupIo, GroupMemory, GroupNetwork, GroupPidCounters, GroupTimers, OsLimit,
    Process, ProcessInfo, Stdio,
};
use crate::{Error, Result};

use std::sync::mpsc::{channel, Receiver, Sender};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

/// An action that is performed when the process terminates.
pub trait OnTerminate: Send {
    fn on_terminate(&mut self);
}

/// Describes the termination reason for a process.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum TerminationReason {
    WallClockTimeLimitExceeded,
    IdleTimeLimitExceeded,
    UserTimeLimitExceeded,
    WriteLimitExceeded,
    MemoryLimitExceeded,
    ProcessLimitExceeded,
    ActiveProcessLimitExceeded,
    ActiveNetworkConnectionLimitExceeded,
    TerminatedByRunner,
}

#[derive(Copy, Clone, Debug)]
pub struct IdleTimeLimit {
    pub total_idle_time: Duration,
    pub cpu_load_threshold: f64,
}

/// The limits that are imposed on a process group.
#[derive(Copy, Clone, Debug)]
pub struct ResourceLimits {
    pub idle_time: Option<IdleTimeLimit>,
    /// The maximum allowed amount of time for a process group.
    pub wall_clock_time: Option<Duration>,
    /// The maximum allowed amount of user-mode execution time for a process group.
    pub total_user_time: Option<Duration>,
    /// The maximum allowed memory usage, in bytes.
    pub max_memory_usage: Option<u64>,
    /// The maximum allowed amount of bytes written by a process group.
    pub total_bytes_written: Option<u64>,
    /// The maximum allowed number of processes created.
    pub total_processes_created: Option<usize>,
    /// The maximum allowed number of active processes.
    pub active_processes: Option<usize>,
    /// The maximum allowed number of active network connections.
    pub active_network_connections: Option<usize>,
}

pub enum RunnerMessage {
    Terminate,
    Suspend,
    Resume,
    StopTimeAccounting,
    ResumeTimeAccounting,
    ResetTime,
}

/// Summary information about process's execution.
#[derive(Clone, Debug)]
pub struct Report {
    pub wall_clock_time: Duration,
    pub memory: Option<GroupMemory>,
    pub io: Option<GroupIo>,
    pub timers: Option<GroupTimers>,
    pub pid_counters: Option<GroupPidCounters>,
    pub network: Option<GroupNetwork>,
    pub exit_status: ExitStatus,
    pub termination_reason: Option<TerminationReason>,
}

pub type MessageChannel = (Sender<RunnerMessage>, Receiver<RunnerMessage>);

pub struct SpawnedProgram {
    info: ProcessInfo,
    group: Option<Group>,
    stdio: Option<Stdio>,
    resource_limits: Option<ResourceLimits>,
    monitor_interval: Duration,
    on_terminate: Option<Box<OnTerminate>>,
    wait_for_children: bool,
    msg_channel: MessageChannel,
}

pub struct Runner {
    sender: Sender<RunnerMessage>,
    handle: JoinHandle<Result<Report>>,
}

pub struct Spawner(Vec<Runner>);

struct ProcessMonitor {
    limit_checker: LimitChecker,
    process: Process,
    creation_time: Instant,
    term_reason: Option<TerminationReason>,
    group: Group,
    msg_receiver: Receiver<RunnerMessage>,
    monitor_interval: Duration,
    wait_for_children: bool,
    on_terminate: Option<Box<OnTerminate>>,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            wall_clock_time: None,
            idle_time: None,
            total_user_time: None,
            max_memory_usage: None,
            total_bytes_written: None,
            total_processes_created: None,
            active_processes: None,
            active_network_connections: None,
        }
    }
}

impl SpawnedProgram {
    pub fn new(info: ProcessInfo) -> Self {
        Self {
            info: info,
            group: None,
            stdio: None,
            resource_limits: None,
            monitor_interval: Duration::from_millis(1),
            on_terminate: None,
            wait_for_children: false,
            msg_channel: channel(),
        }
    }

    pub fn group(&mut self, group: Group) -> &mut Self {
        self.group = Some(group);
        self
    }

    pub fn resource_limits(&mut self, resource_limits: ResourceLimits) -> &mut Self {
        self.resource_limits = Some(resource_limits);
        self
    }

    pub fn monitor_interval(&mut self, monitor_interval: Duration) -> &mut Self {
        self.monitor_interval = monitor_interval;
        self
    }

    pub fn on_terminate<T>(&mut self, on_terminate: T) -> &mut Self
    where
        T: OnTerminate + 'static,
    {
        self.on_terminate = Some(Box::new(on_terminate));
        self
    }

    pub fn stdio(&mut self, stdio: Stdio) -> &mut Self {
        self.stdio = Some(stdio);
        self
    }

    pub fn wait_for_children(&mut self, wait: bool) -> &mut Self {
        self.wait_for_children = wait;
        self
    }

    pub fn msg_channel(&mut self, channel: MessageChannel) -> &mut Self {
        self.msg_channel = channel;
        self
    }
}

impl Runner {
    pub fn sender(&self) -> &Sender<RunnerMessage> {
        &self.sender
    }
}

impl Spawner {
    pub fn spawn<I>(programs: I) -> Self
    where
        I: IntoIterator<Item = SpawnedProgram>,
    {
        Self(
            programs
                .into_iter()
                .map(|prog| Runner {
                    sender: prog.msg_channel.0.clone(),
                    handle: thread::spawn(move || {
                        ProcessMonitor::new(prog).and_then(|mut pm| pm.start_monitoring())
                    }),
                })
                .collect(),
        )
    }

    pub fn runners(&self) -> &[Runner] {
        &self.0
    }

    pub fn wait(self) -> Vec<Result<Report>> {
        self.0
            .into_iter()
            .map(|runner| {
                runner
                    .handle
                    .join()
                    .unwrap_or(Err(Error::from("Runner thread panicked")))
            })
            .collect()
    }
}

impl ProcessMonitor {
    fn new(program: SpawnedProgram) -> Result<Self> {
        let msg_receiver = program.msg_channel.1;
        let limits = program.resource_limits.unwrap_or_default();
        let monitor_interval = program.monitor_interval;
        let wait_for_children = program.wait_for_children;
        let on_terminate = program.on_terminate;
        let mut group = match program.group {
            Some(g) => g,
            None => Group::new()?,
        };
        let limit_checker = LimitChecker::new(
            limits,
            EnabledOsLimits {
                memory: limits
                    .max_memory_usage
                    .map(|limit| group.set_os_limit(OsLimit::Memory, limit))
                    .transpose()?
                    .unwrap_or(false),
                active_process: limits
                    .active_processes
                    .map(|limit| group.set_os_limit(OsLimit::ActiveProcess, limit as u64))
                    .transpose()?
                    .unwrap_or(false),
            },
        );
        Process::spawn_in_group(
            program.info,
            match program.stdio {
                Some(stdio) => stdio,
                None => Stdio {
                    stdin: ReadPipe::null()?,
                    stdout: WritePipe::null()?,
                    stderr: WritePipe::null()?,
                },
            },
            &mut group,
        )
        .map(|ps| Self {
            limit_checker: limit_checker,
            process: ps,
            creation_time: Instant::now(),
            term_reason: None,
            group: group,
            msg_receiver: msg_receiver,
            monitor_interval: monitor_interval,
            wait_for_children: wait_for_children,
            on_terminate: on_terminate,
        })
    }

    fn start_monitoring(&mut self) -> Result<Report> {
        loop {
            if let Some(report) = self.get_report()? {
                return Ok(report);
            }
            if let Some(tr) = self.limit_checker.check(&mut self.group)? {
                self.group.terminate()?;
                self.term_reason = Some(tr);
            }
            self.handle_messages()?;
            thread::sleep(self.monitor_interval);
        }
    }

    fn get_report(&mut self) -> Result<Option<Report>> {
        let exit_status = match self.process.exit_status()? {
            Some(status) => status,
            None => return Ok(None),
        };

        let pid_counters = self.group.pid_counters()?;

        if self.wait_for_children
            && pid_counters.is_some()
            && pid_counters.unwrap().active_processes != 0
        {
            return Ok(None);
        }

        if self.term_reason.is_none() {
            self.term_reason = self.limit_checker.check(&mut self.group)?;
        }

        Ok(Some(Report {
            wall_clock_time: self.creation_time.elapsed(),
            memory: self.group.memory()?,
            io: self.group.io()?,
            timers: self.group.timers()?,
            pid_counters: pid_counters,
            network: self.group.network()?,
            exit_status: exit_status,
            termination_reason: self.term_reason,
        }))
    }

    fn handle_messages(&mut self) -> Result<()> {
        for msg in self.msg_receiver.try_iter().take(10) {
            match msg {
                RunnerMessage::Terminate => {
                    self.group.terminate()?;
                    self.term_reason = Some(TerminationReason::TerminatedByRunner);
                }
                RunnerMessage::Suspend => {
                    if self.process.exit_status()?.is_none() {
                        self.process.suspend()?;
                    }
                }
                RunnerMessage::Resume => {
                    if self.process.exit_status()?.is_none() {
                        self.process.resume()?;
                    }
                }
                RunnerMessage::ResetTime => self.limit_checker.reset_time(),
                RunnerMessage::StopTimeAccounting => self.limit_checker.stop_time_accounting(),
                RunnerMessage::ResumeTimeAccounting => self.limit_checker.resume_time_accounting(),
            }
        }

        Ok(())
    }
}

impl Drop for ProcessMonitor {
    fn drop(&mut self) {
        let _ = self.group.terminate();
        if let Some(mut handler) = self.on_terminate.take() {
            handler.on_terminate();
        }
    }
}
