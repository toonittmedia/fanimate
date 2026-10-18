//! The render queue: jobs waiting to be exported, kept in a folder so the
//! editor and the queue window (separate processes) share them.
//!
//! ```text
//! <root>/
//!   order.json           job ids, in the order they run
//!   order.lock           held while order.json is read and rewritten
//!   runner.lock          held by the queue window while it is open
//!   jobs/<id>/
//!     job.json           what to export, and where to
//!     status.json        how far it got
//!     project.fnm/       the project as it was when queued
//! ```
//!
//! A job keeps the project as it was when queued, so editing on while it
//! waits does not change what is exported.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use fanimate_core::Document;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::job::Settings;

/// One export waiting, running or finished.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: String,
    /// The project's name, for showing.
    pub name: String,
    pub output: PathBuf,
    pub settings: Settings,
    /// When it was queued, in seconds since 1970.
    pub queued: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum State {
    #[default]
    Waiting,
    Rendering,
    Done,
    Failed,
    Cancelled,
}

impl State {
    pub fn is_finished(self) -> bool {
        matches!(self, State::Done | State::Failed | State::Cancelled)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Status {
    pub state: State,
    /// Frames done, of `total`.
    pub done: u32,
    pub total: u32,
    /// Why it failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

impl Status {
    /// How far along, from 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.state == State::Done {
            1.0
        } else if self.total == 0 {
            0.0
        } else {
            self.done as f32 / self.total as f32
        }
    }
}

/// A job with its latest status.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub job: Job,
    pub status: Status,
}

/// Held by the process that runs jobs; released when dropped or when the
/// process ends, however it ends.
pub struct RunnerLock {
    _file: File,
}

#[derive(Clone, Debug)]
pub struct Queue {
    root: PathBuf,
}

impl Queue {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn dir(&self, id: &str) -> PathBuf {
        self.root.join("jobs").join(id)
    }

    /// The copy of the project job `id` exports.
    pub fn project(&self, id: &str) -> PathBuf {
        self.dir(id).join("project.fnm")
    }

    /// Queue an export of `doc` (its timeline, not an open action) at the
    /// end. Returns the job's id.
    pub fn add(&self, doc: &Document, output: &Path, settings: &Settings) -> std::io::Result<String> {
        static ADDED: AtomicU32 = AtomicU32::new(0);
        let queued = now();
        let id = format!(
            "{queued}-{}-{}",
            std::process::id(),
            ADDED.fetch_add(1, Ordering::Relaxed)
        );
        let dir = self.dir(&id);
        fs::create_dir_all(&dir)?;
        let written = (|| {
            fanimate_fnm::save_folder(doc, self.project(&id)).map_err(std::io::Error::other)?;
            let job = Job {
                id: id.clone(),
                name: doc.name.clone(),
                output: output.to_path_buf(),
                settings: settings.clone(),
                queued,
            };
            write_json(&dir.join("job.json"), &job)?;
            write_json(
                &dir.join("status.json"),
                &Status {
                    total: settings.sanitized().frame_count(),
                    ..Default::default()
                },
            )?;
            self.edit_order(|order| order.push(id.clone()))
        })();
        if let Err(e) = written {
            let _ = fs::remove_dir_all(&dir);
            return Err(e);
        }
        Ok(id)
    }

    /// Every job, in the order they run. Jobs that cannot be read are
    /// left out.
    pub fn jobs(&self) -> Vec<Entry> {
        self.order()
            .into_iter()
            .filter_map(|id| {
                let job: Job = read_json(&self.dir(&id).join("job.json"))?;
                let status = read_json(&self.dir(&id).join("status.json")).unwrap_or_default();
                Some(Entry { job, status })
            })
            .collect()
    }

    /// The first job still waiting.
    pub fn next_waiting(&self) -> Option<Job> {
        self.jobs()
            .into_iter()
            .find(|e| e.status.state == State::Waiting)
            .map(|e| e.job)
    }

    pub fn set_status(&self, id: &str, status: &Status) -> std::io::Result<()> {
        write_json(&self.dir(id).join("status.json"), status)
    }

    /// Move job `id` by `by` places (negative is earlier).
    pub fn move_by(&self, id: &str, by: isize) -> std::io::Result<()> {
        self.edit_order(|order| {
            if let Some(i) = order.iter().position(|j| j == id) {
                let to = (i as isize + by).clamp(0, order.len() as isize - 1) as usize;
                let job = order.remove(i);
                order.insert(to, job);
            }
        })
    }

    /// Take job `id` off the queue and delete its copy of the project. The
    /// exported file stays.
    pub fn remove(&self, id: &str) -> std::io::Result<()> {
        self.edit_order(|order| order.retain(|j| j != id))?;
        match fs::remove_dir_all(self.dir(id)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        }
    }

    /// Become the process that runs jobs, unless another one is.
    pub fn try_run(&self) -> Option<RunnerLock> {
        fs::create_dir_all(&self.root).ok()?;
        let file = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join("runner.lock"))
            .ok()?;
        file.try_lock().ok()?;
        Some(RunnerLock { _file: file })
    }

    /// Whether a queue window is open to run the jobs.
    pub fn has_runner(&self) -> bool {
        self.try_run().is_none() && self.root.join("runner.lock").exists()
    }

    /// Jobs a runner left half done when it stopped (closed or crashed)
    /// go back to waiting, and a crashed or killed runner's unfinished
    /// files are removed.
    pub fn recover(&self) {
        for e in self.jobs() {
            if e.status.state == State::Rendering {
                crate::run::remove_parts(&e.job.output, e.job.settings.format);
                let _ = self.set_status(
                    &e.job.id,
                    &Status {
                        total: e.status.total,
                        ..Default::default()
                    },
                );
            }
        }
    }

    fn order(&self) -> Vec<String> {
        read_json(&self.root.join("order.json")).unwrap_or_default()
    }

    /// Change the order under its lock, so two processes never lose each
    /// other's changes.
    fn edit_order(&self, edit: impl FnOnce(&mut Vec<String>)) -> std::io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(self.root.join("order.lock"))?;
        lock.lock()?;
        let mut order = self.order();
        edit(&mut order);
        // jobs whose folder is gone are dropped
        order.retain(|id| self.dir(id).join("job.json").is_file());
        write_json(&self.root.join("order.json"), &order)
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn read_json<T: DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&fs::read(path).ok()?).ok()
}

/// Write via a temporary file, so a reader never sees half a file.
fn write_json<T: Serialize>(path: &Path, value: &T) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(
        &tmp,
        serde_json::to_vec_pretty(value).map_err(std::io::Error::other)?,
    )?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::Format;
    use fanimate_core::samples;

    fn queue(name: &str) -> Queue {
        let dir = std::env::temp_dir().join(format!("fanimate-queue-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        Queue::new(dir)
    }

    fn add(q: &Queue, out: &str) -> String {
        let doc = samples::hello();
        q.add(&doc, Path::new(out), &Settings::for_doc(&doc, Format::Gif))
            .unwrap()
    }

    #[test]
    fn jobs_run_in_order_and_can_be_reordered() {
        let q = queue("order");
        let (a, b, c) = (add(&q, "a.gif"), add(&q, "b.gif"), add(&q, "c.gif"));
        let ids = |q: &Queue| q.jobs().into_iter().map(|e| e.job.id).collect::<Vec<_>>();
        assert_eq!(ids(&q), [a.clone(), b.clone(), c.clone()]);
        q.move_by(&c, -1).unwrap();
        assert_eq!(ids(&q), [a.clone(), c.clone(), b.clone()]);
        q.move_by(&a, 10).unwrap();
        assert_eq!(ids(&q), [c.clone(), b.clone(), a.clone()]);
        assert_eq!(q.next_waiting().unwrap().id, c);
        q.set_status(
            &c,
            &Status {
                state: State::Done,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(q.next_waiting().unwrap().id, b);
        q.remove(&b).unwrap();
        assert_eq!(ids(&q), [c, a.clone()]);
        assert!(!q.project(&b).exists());
        // the queued copy opens, as the project was
        let (doc, report) = fanimate_fnm::load(q.project(&a)).unwrap();
        assert!(report.is_clean());
        assert_eq!(doc.layers, samples::hello().layers);
        fs::remove_dir_all(q.root()).unwrap();
    }

    #[test]
    fn one_runner_at_a_time_and_half_done_jobs_wait_again() {
        let q = queue("runner");
        let id = add(&q, "a.gif");
        let lock = q.try_run().unwrap();
        assert!(q.try_run().is_none());
        assert!(q.has_runner());
        q.set_status(
            &id,
            &Status {
                state: State::Rendering,
                done: 5,
                total: 48,
                message: None,
            },
        )
        .unwrap();
        drop(lock);
        assert!(!q.has_runner());
        q.recover();
        let status = &q.jobs()[0].status;
        assert_eq!((status.state, status.done, status.total), (State::Waiting, 0, 48));
        fs::remove_dir_all(q.root()).unwrap();
    }

    /// A runner killed mid-render (Task Manager, a crash) cannot remove its
    /// `.part` files; the next runner does, and leaves finished files be.
    #[test]
    fn recovering_removes_a_killed_runners_unfinished_files() {
        let q = queue("killed");
        fs::create_dir_all(q.root()).unwrap();
        let doc = samples::hello();
        let rendering = Status {
            state: State::Rendering,
            done: 5,
            total: 48,
            message: None,
        };
        let video = q.root().join("scene.mp4");
        let frames = q.root().join("scene.png");
        for (out, format) in [(&video, Format::Mp4), (&frames, Format::Png)] {
            let id = q.add(&doc, out, &Settings::for_doc(&doc, format)).unwrap();
            q.set_status(&id, &rendering).unwrap();
        }
        let keep = ["scene.mp4", "scene_0000.png", "other_0001.png.part"];
        let gone = ["scene.mp4.part", "scene_0001.png.part"];
        for name in keep.iter().chain(&gone) {
            fs::write(q.root().join(name), b"x").unwrap();
        }
        q.recover();
        for name in keep {
            assert!(q.root().join(name).exists(), "{name} is kept");
        }
        for name in gone {
            assert!(!q.root().join(name).exists(), "{name} is removed");
        }
        fs::remove_dir_all(q.root()).unwrap();
    }

    #[test]
    fn unreadable_jobs_are_skipped() {
        let q = queue("broken");
        let id = add(&q, "a.gif");
        fs::write(q.dir(&id).join("job.json"), b"{").unwrap();
        assert!(q.jobs().is_empty());
        assert!(q.next_waiting().is_none());
        fs::remove_dir_all(q.root()).unwrap();
    }
}
