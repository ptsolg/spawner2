use crate::Result;
use std::io::{self, Read, Write};
use std::path::Path;
use sys::pipe as pipe_impl;
use sys::IntoInner;

/// A reference to the reading end of a pipe or to the file opened in read mode.
/// Returned by [`create`] function or by [`ReadPipe::open`] method.
///
/// [`create`]: fn.create.html
/// [`ReadPipe::open`]: struct.ReadPipe.html#method.open
pub struct ReadPipe(pipe_impl::ReadPipe);

/// A reference to the writing end of a pipe or to the file opened in write mode.
/// Returned by [`create`] function or by [`WritePipe::open`] method.
///
/// [`create`]: fn.create.html
/// [`WritePipe::open`]: struct.WritePipe.html#method.open
pub struct WritePipe(pipe_impl::WritePipe);

/// Describes the sharing mode of a file.
#[derive(PartialEq)]
pub enum ShareMode {
    /// File can be opened by other processes.
    Shared,
    /// File cannot be opened by other processes.
    Exclusive,
}

/// Creates a new pipe returning the [`ReadPipe`] and [`WritePipe`] pair.
///
/// [`ReadPipe`]: struct.ReadPipe.html
/// [`WritePipe`]: struct.WritePipe.html
pub fn create() -> Result<(ReadPipe, WritePipe)> {
    let (r, w) = pipe_impl::create()?;
    Ok((ReadPipe(r), WritePipe(w)))
}

impl ReadPipe {
    /// Opens a file in read-only mode.
    pub fn open<P: AsRef<Path>>(path: P, share_mode: ShareMode) -> Result<Self> {
        Ok(Self(pipe_impl::ReadPipe::open(
            path,
            share_mode == ShareMode::Exclusive,
        )?))
    }

    pub fn null() -> Result<Self> {
        Ok(Self(pipe_impl::ReadPipe::null()?))
    }
}

impl IntoInner<pipe_impl::ReadPipe> for ReadPipe {
    fn into_inner(self) -> pipe_impl::ReadPipe {
        self.0
    }
}

impl Read for ReadPipe {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.0.read(buf)
    }
}

impl WritePipe {
    /// Opens a file in write-only mode.
    pub fn open<P: AsRef<Path>>(path: P, share_mode: ShareMode) -> Result<Self> {
        Ok(Self(pipe_impl::WritePipe::open(
            path,
            share_mode == ShareMode::Exclusive,
        )?))
    }

    pub fn null() -> Result<Self> {
        Ok(Self(pipe_impl::WritePipe::null()?))
    }

    pub fn is_file(&self) -> bool {
        self.0.is_file()
    }
}

impl IntoInner<pipe_impl::WritePipe> for WritePipe {
    fn into_inner(self) -> pipe_impl::WritePipe {
        self.0
    }
}

impl Write for WritePipe {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}