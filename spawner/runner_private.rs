use crate::command::{Command, OnTerminate};
use crate::pipe::{ReadPipe, WritePipe};
use crate::runner::{Runner, RunnerReport};
use crate::sys::runner as runner_impl;
use crate::sys::IntoInner;
use crate::Result;

pub struct RunnerThread(runner_impl::RunnerThread);

pub struct ProcessStdio {
    pub stdin: ReadPipe,
    pub stdout: WritePipe,
    pub stderr: WritePipe,
}

pub fn spawn(
    cmd: Command,
    stdio: ProcessStdio,
    on_terminate: Option<Box<OnTerminate>>,
) -> Result<RunnerThread> {
    runner_impl::spawn(
        cmd,
        runner_impl::ProcessStdio {
            stdin: stdio.stdin.into_inner(),
            stdout: stdio.stdout.into_inner(),
            stderr: stdio.stderr.into_inner(),
        },
        on_terminate,
    )
    .map(|thread| RunnerThread(thread))
}

impl RunnerThread {
    pub fn runner(&self) -> &Runner {
        self.0.runner()
    }

    pub fn join(self) -> Result<RunnerReport> {
        self.0.join()
    }
}
