use rand::distributions::Alphanumeric;
use rand::{thread_rng, Rng};
use std::fs;
use std::io::prelude::*;
use std::iter;
use std::ops::{Add, Sub};
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

pub struct TmpDir {
    dir: PathBuf,
}

impl TmpDir {
    pub fn new() -> Self {
        let mut rng = thread_rng();
        let name: String = iter::repeat(())
            .map(|()| rng.sample(Alphanumeric))
            .take(7)
            .collect();

        let dir = PathBuf::from(name);
        fs::create_dir(dir.as_path()).unwrap();

        Self {
            dir: dir.canonicalize().unwrap(),
        }
    }

    pub fn file<P: AsRef<Path>>(&self, filename: P) -> String {
        let mut path = self.dir.clone();
        path.push(filename);
        path.to_str().unwrap().to_string()
    }
}

impl Drop for TmpDir {
    fn drop(&mut self) {
        // The directory might be locked by another programm.
        for _ in 0..5000 {
            match fs::remove_dir_all(self.dir.as_path()) {
                Err(_) => thread::sleep(Duration::from_millis(1)),
                Ok(_) => break,
            }
        }
    }
}

#[macro_export]
macro_rules! exe {
    ($s:expr) => {
        concat!("../target/debug/", $s, ".exe")
    };
}

pub fn approx_eq<T>(a: T, b: T, diff: T) -> bool
where
    T: Add<Output = T> + Sub<Output = T> + PartialOrd + Copy,
{
    (a > (b - diff)) && (a < (b + diff))
}

#[macro_export]
macro_rules! assert_approx_eq {
    ($a:expr, $b:expr, $diff:expr) => {
        assert!($crate::common::approx_eq($a, $b, $diff))
    };
}

pub fn read_all<P: AsRef<Path>>(path: P) -> String {
    let mut result = String::new();
    let _ = fs::File::open(path).unwrap().read_to_string(&mut result);
    result
}

pub fn write_all<P, S>(filename: P, data: S)
where
    P: AsRef<Path>,
    S: AsRef<str>,
{
    let mut file = fs::File::create(filename).unwrap();
    let _ = write!(file, "{}", data.as_ref());
}
