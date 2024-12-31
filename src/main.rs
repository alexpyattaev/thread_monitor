use std::{
    collections::{HashMap, hash_map::Entry},
    path::Display,
    process::{self, exit},
};

use clap::{Parser, Subcommand};
use procfs::process::{Process, Stat};

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    pid: i32,
    #[arg(default_value_t = 400)]
    sampling_interval_ms: u32,
}

#[derive(Default)]
struct Counter {
    value: u64,
    samples: u64,
}

impl std::fmt::Debug for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}
impl std::fmt::Display for Counter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (x, var) = self.get();
        write!(f, "{x}±{var}")
    }
}

impl Counter {
    fn sample(&mut self, v: u64) {
        self.value += v;
        self.samples += 1;
    }

    fn get(&self) -> (f32, f32) {
        if self.samples == 0 {
            return (f32::NAN, f32::INFINITY);
        }
        ((self.value as f64 / self.samples as f64) as f32, 0.0)
    }
}

#[derive(Default, Debug)]
struct ThreadStats {
    user_time: Counter,
    sys_time: Counter,
    io_time: Counter,
    major_page_faults: Counter,
    minor_page_faults: Counter,
}
impl ThreadStats {
    fn update_from_stat(&mut self, stat: Stat) -> anyhow::Result<()> {
        self.sys_time.sample(stat.stime);
        self.user_time.sample(stat.utime);
        self.io_time.sample(
            stat.delayacct_blkio_ticks
                .ok_or(anyhow::anyhow!("Blkio accounting borken"))?,
        );
        self.major_page_faults.sample(stat.majflt);
        self.minor_page_faults.sample(stat.minflt);
        Ok(())
    }
}
fn get_epoch_progress() -> anyhow::Result<f64> {
    let out = std::process::Command::new("solana").arg("slot").output()?;
    let sp: f64 = String::from_utf8(out.stdout)?.parse()?;
    Ok(sp / 432000.0)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let root = Process::new(cli.pid)?;
    if !root.is_alive() {
        println!("Specified process is not alive!");
        exit(1);
    }

    let mut mon_data = HashMap::<String, ThreadStats>::new();

    let start_point = get_epoch_progress()?;
    let end_point = start_point.ceil() + 0.1;
    use tqdm::pbar;
    let mut pbar = pbar(Some(100));
    loop {
        let progress = get_epoch_progress()?;
        let remaining = end_point - progress;
        if remaining > 0.0 {
            pbar.update(((1.0 - remaining) * 100.0) as usize)?;
        } else {
            break;
        }

        for task in root.tasks()?.flatten() {
            let mut stat = task.stat()?;
            let mut name = String::new();
            std::mem::swap(&mut stat.comm, &mut name);
            match mon_data.entry(name) {
                Entry::Occupied(mut entry) => {
                    entry.get_mut().update_from_stat(stat)?;
                }
                Entry::Vacant(vacant_entry) => {
                    let mut entry = vacant_entry.insert_entry(ThreadStats::default());
                    entry.get_mut().update_from_stat(stat)?;
                }
            }
        }
    }
    dbg!(&mon_data);
    Ok(())
}
