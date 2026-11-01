use std::path::{Path, PathBuf};

use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};

use super::media_info::under_root;
use super::{Library, new_id, now_ms};
use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionProfile {
    Remux,

    AudioFix,

    Standard,
}

impl ConversionProfile {
    pub const ALL: [Self; 3] = [Self::Remux, Self::AudioFix, Self::Standard];

    pub fn id(self) -> &'static str {
        match self {
            Self::Remux => "remux",
            Self::AudioFix => "audio_fix",
            Self::Standard => "standard",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Remux => "remux",
            Self::AudioFix => "audio fix",
            Self::Standard => "h264",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Remux => "Repackage the streams into MP4 without touching a single frame.",
            Self::AudioFix => {
                "Keep the video as it is and re-encode only the audio to AAC, for players \
                 that refuse the current audio codec."
            }
            Self::Standard => {
                "Re-encode video to H.264 at up to 1080p and audio to AAC. Slower, and the \
                 result is not the original quality."
            }
        }
    }

    pub fn extension(self) -> &'static str {
        "mp4"
    }

    pub fn from_id(value: &str) -> Option<Self> {
        match value {
            "remux" => Some(Self::Remux),
            "audio_fix" => Some(Self::AudioFix),
            "standard" => Some(Self::Standard),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ConversionStatus {
    Queued,

    Running,

    Verifying,

    Done,

    Failed,

    Cancelled,
}

impl ConversionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Verifying => "verifying",
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "verifying" => Self::Verifying,
            "done" => Self::Done,
            "failed" => Self::Failed,
            "cancelled" => Self::Cancelled,
            _ => Self::Queued,
        }
    }

    pub fn is_active(self) -> bool {
        matches!(self, Self::Queued | Self::Running | Self::Verifying)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Queued => "waiting",
            Self::Running => "converting",
            Self::Verifying => "checking the result",
            Self::Done => "finished",
            Self::Failed => "failed",
            Self::Cancelled => "stopped",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionPlan {
    pub file_id: String,

    pub title: String,

    pub profile: ConversionProfile,

    pub source_path: PathBuf,

    pub output_path: PathBuf,

    pub source_size: i64,

    pub source_modified_ms: i64,

    pub duration_ms: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversionJob {
    pub id: String,

    pub file_id: String,

    pub title: String,

    pub profile: ConversionProfile,

    pub status: ConversionStatus,

    pub progress_percent: u8,

    pub message: String,

    pub source_path: String,

    pub output_path: String,

    pub source_size: i64,

    pub source_modified_ms: i64,

    pub output_bytes: i64,

    pub created_ms: i64,

    pub updated_ms: i64,
}

impl ConversionJob {
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    pub fn summary(&self) -> String {
        let mut parts = vec![
            self.title.clone(),
            self.profile.label().to_string(),
            self.status.label().to_string(),
        ];
        if self.status.is_active() {
            parts.push(format!("{}%", self.progress_percent));
        }
        if !self.message.trim().is_empty() {
            parts.push(self.message.clone());
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputFacts {
    pub duration_ms: i64,

    pub has_video: bool,

    pub has_audio: bool,
}

struct SourceFacts {
    modified_ms: i64,
    relative_path: String,
    available: bool,
    root_path: String,
    duration_ms: i64,
    title: String,
}

impl Library {
    pub fn plan_conversion(
        &self,
        file_id: &str,
        profile: ConversionProfile,
        output_folder: &Path,
    ) -> Result<ConversionPlan> {
        let facts = self.conversion_source(file_id)?.ok_or_else(unknown_file)?;
        if !facts.available {
            return Err(CoreError::library(
                "conversion_source_unavailable",
                "That file cannot be read right now, so there is nothing to convert. \
                 Relink its folder and rescan first.",
            ));
        }
        let source_path =
            under_root(&facts.root_path, &facts.relative_path).ok_or_else(unknown_file)?;
        let metadata = std::fs::metadata(&source_path).map_err(|error| {
            CoreError::library(
                "conversion_source_unavailable",
                format!("That file cannot be read: {}.", error.kind()),
            )
        })?;
        if !metadata.is_file() {
            return Err(unknown_file());
        }

        let folder = absolute_folder(output_folder)?;
        let output_path = unique_output(&folder, &source_path, profile)?;
        if same_file(&source_path, &output_path) {
            return Err(CoreError::library(
                "conversion_overwrites_source",
                "The converted file would replace the original. Choose another output folder.",
            ));
        }

        Ok(ConversionPlan {
            file_id: file_id.to_string(),
            title: facts.title,
            profile,
            source_path,
            output_path,
            source_size: metadata.len() as i64,
            source_modified_ms: file_modified_ms(&metadata).unwrap_or(facts.modified_ms),
            duration_ms: facts.duration_ms,
        })
    }

    pub fn begin_conversion(&self, plan: &ConversionPlan) -> Result<ConversionJob> {
        let connection = self.connection();
        let active: Option<String> = connection
            .query_row(
                "SELECT id FROM conversion_jobs
                 WHERE file_id = ?1 AND status IN ('queued','running','verifying')",
                [&plan.file_id],
                |row| row.get(0),
            )
            .optional()?;
        if active.is_some() {
            return Err(CoreError::library(
                "conversion_in_progress",
                "That file is already being converted.",
            ));
        }
        if plan.output_path.exists() {
            return Err(CoreError::library(
                "conversion_output_exists",
                "A file with that name already exists. Nothing was overwritten.",
            ));
        }

        let job = ConversionJob {
            id: new_id(),
            file_id: plan.file_id.clone(),
            title: plan.title.clone(),
            profile: plan.profile,
            status: ConversionStatus::Queued,
            progress_percent: 0,
            message: String::new(),
            source_path: plan.source_path.to_string_lossy().to_string(),
            output_path: plan.output_path.to_string_lossy().to_string(),
            source_size: plan.source_size,
            source_modified_ms: plan.source_modified_ms,
            output_bytes: 0,
            created_ms: now_ms(),
            updated_ms: now_ms(),
        };

        connection.execute(
            "INSERT INTO conversion_jobs(id, file_id, title, profile, status, progress, message,
               source_path, output_path, source_size, source_modified_ms, output_bytes,
               created_ms, updated_ms)
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                job.id,
                job.file_id,
                job.title,
                job.profile.id(),
                job.status.as_str(),
                job.progress_percent,
                job.message,
                job.source_path,
                job.output_path,
                job.source_size,
                job.source_modified_ms,
                job.output_bytes,
                job.created_ms,
                job.updated_ms,
            ],
        )?;
        Ok(job)
    }

    pub fn set_conversion_progress(&self, id: &str, percent: u8) -> Result<()> {
        let percent = percent.min(100);
        self.connection().execute(
            "UPDATE conversion_jobs
             SET progress = MAX(progress, ?2), status = 'running', updated_ms = ?3
             WHERE id = ?1 AND status IN ('queued','running')",
            params![id, percent, now_ms()],
        )?;
        Ok(())
    }

    pub fn set_conversion_status(&self, id: &str, status: ConversionStatus) -> Result<()> {
        if !status.is_active() {
            return Err(CoreError::library(
                "conversion_invalid_status",
                "A finished conversion cannot go back to waiting.",
            ));
        }
        let connection = self.connection();
        let current: Option<String> = connection
            .query_row(
                "SELECT status FROM conversion_jobs WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        match current {
            None => {
                return Err(CoreError::library(
                    "conversion_missing",
                    "That conversion is no longer recorded.",
                ));
            }
            Some(current) if !ConversionStatus::parse(&current).is_active() => {
                return Err(CoreError::library(
                    "conversion_invalid_status",
                    "A finished conversion cannot be started again.",
                ));
            }
            Some(_) => {}
        }
        connection.execute(
            "UPDATE conversion_jobs SET status = ?2, updated_ms = ?3
             WHERE id = ?1 AND status IN ('queued','running','verifying')",
            params![id, status.as_str(), now_ms()],
        )?;
        Ok(())
    }

    pub fn finish_conversion(
        &self,
        id: &str,
        status: ConversionStatus,
        message: &str,
        output_bytes: i64,
    ) -> Result<()> {
        if status.is_active() {
            return Err(CoreError::library(
                "conversion_invalid_status",
                "Only a finished conversion can be recorded as finished.",
            ));
        }
        self.connection().execute(
            "UPDATE conversion_jobs
             SET status = ?2, message = ?3, output_bytes = ?4, updated_ms = ?5,
                 progress = CASE WHEN ?2 = 'done' THEN 100 ELSE progress END
             WHERE id = ?1",
            params![
                id,
                status.as_str(),
                message.chars().take(300).collect::<String>(),
                output_bytes.max(0),
                now_ms(),
            ],
        )?;
        Ok(())
    }

    pub fn cancel_conversion(&self, id: &str) -> Result<bool> {
        let changed = self.connection().execute(
            "UPDATE conversion_jobs
             SET status = 'cancelled', message = 'Stopped at your request.', updated_ms = ?2
             WHERE id = ?1 AND status IN ('queued','running','verifying')",
            params![id, now_ms()],
        )?;
        Ok(changed > 0)
    }

    pub fn recent_conversions(&self, limit: i64) -> Result<Vec<ConversionJob>> {
        let connection = self.connection();
        let mut statement = connection.prepare(
            "SELECT id, file_id, title, profile, status, progress, message, source_path,
                    output_path, source_size, source_modified_ms, output_bytes, created_ms,
                    updated_ms
             FROM conversion_jobs
             ORDER BY created_ms DESC, id DESC
             LIMIT ?1",
        )?;
        let rows = statement.query_map([limit.clamp(1, 100)], |row| {
            Ok(ConversionJob {
                id: row.get(0)?,
                file_id: row.get(1)?,
                title: row.get(2)?,
                profile: ConversionProfile::from_id(&row.get::<_, String>(3)?)
                    .unwrap_or(ConversionProfile::Remux),
                status: ConversionStatus::parse(&row.get::<_, String>(4)?),
                progress_percent: row.get::<_, i64>(5)?.clamp(0, 100) as u8,
                message: row.get(6)?,
                source_path: row.get(7)?,
                output_path: row.get(8)?,
                source_size: row.get(9)?,
                source_modified_ms: row.get(10)?,
                output_bytes: row.get(11)?,
                created_ms: row.get(12)?,
                updated_ms: row.get(13)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn conversion_for_file(&self, file_id: &str) -> Result<Option<ConversionJob>> {
        Ok(self
            .recent_conversions(100)?
            .into_iter()
            .find(|job| job.file_id == file_id))
    }

    pub fn active_conversion(&self) -> Result<Option<ConversionJob>> {
        Ok(self
            .recent_conversions(100)?
            .into_iter()
            .find(ConversionJob::is_active))
    }

    fn conversion_source(&self, file_id: &str) -> Result<Option<SourceFacts>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT f.modified_ms, f.relative_path, f.available, s.root_path,
                        e.duration_ms, w.title
                 FROM files f
                 JOIN sources s ON s.id = f.source_id
                 JOIN editions e ON e.id = f.edition_id
                 JOIN works w ON w.id = e.work_id
                 WHERE f.id = ?1",
                [file_id],
                |row| {
                    Ok(SourceFacts {
                        modified_ms: row.get(0)?,
                        relative_path: row.get(1)?,
                        available: row.get::<_, i64>(2)? == 1,
                        root_path: row.get(3)?,
                        duration_ms: row.get(4)?,
                        title: row.get(5)?,
                    })
                },
            )
            .optional()?)
    }
}

pub fn verify_output(
    source_duration_ms: i64,
    source_has_audio: bool,
    output: &OutputFacts,
) -> Result<()> {
    if !output.has_video {
        return Err(CoreError::library(
            "conversion_verify_failed",
            "The converted file has no video, so it was not kept.",
        ));
    }
    if source_has_audio && !output.has_audio {
        return Err(CoreError::library(
            "conversion_verify_failed",
            "The converted file lost its audio, so it was not kept.",
        ));
    }
    if output.duration_ms <= 0 {
        return Err(CoreError::library(
            "conversion_verify_failed",
            "The converted file reports no duration, so it was not kept.",
        ));
    }
    if source_duration_ms > 0 {
        let tolerance = (source_duration_ms / 100).max(2_000);
        let difference = (output.duration_ms - source_duration_ms).abs();
        if difference > tolerance {
            return Err(CoreError::library(
                "conversion_verify_failed",
                format!(
                    "The converted copy runs {} s against the original's {} s, which is too \
                     different to call a copy.",
                    output.duration_ms / 1_000,
                    source_duration_ms / 1_000
                ),
            ));
        }
    }
    Ok(())
}

fn file_modified_ms(metadata: &std::fs::Metadata) -> Option<i64> {
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|elapsed| elapsed.as_millis() as i64)
}

fn unknown_file() -> CoreError {
    CoreError::library(
        "conversion_source_missing",
        "That file is no longer in the library.",
    )
}

fn absolute_folder(folder: &Path) -> Result<PathBuf> {
    if !folder.is_absolute() {
        return Err(CoreError::library(
            "conversion_folder_invalid",
            "Choose an absolute folder for converted files.",
        ));
    }
    if folder.exists() {
        let metadata = std::fs::metadata(folder)?;
        if !metadata.is_dir() {
            return Err(CoreError::library(
                "conversion_folder_invalid",
                "That path is a file, not a folder.",
            ));
        }
        if metadata.permissions().readonly() {
            return Err(CoreError::library(
                "conversion_folder_readonly",
                "That folder is read-only. Choose a folder you can write to; a read-only \
                 drive is never written to.",
            ));
        }
    } else {
        std::fs::create_dir_all(folder).map_err(|error| {
            CoreError::library(
                "conversion_folder_invalid",
                format!("That folder could not be created: {}.", error.kind()),
            )
        })?;
    }
    Ok(folder.to_path_buf())
}

fn unique_output(folder: &Path, source: &Path, profile: ConversionProfile) -> Result<PathBuf> {
    let stem = source
        .file_stem()
        .map(|stem| stem.to_string_lossy().to_string())
        .filter(|stem| !stem.trim().is_empty())
        .ok_or_else(|| {
            CoreError::library(
                "conversion_source_invalid",
                "That file has no usable name to build an output name from.",
            )
        })?;
    for attempt in 1..=99 {
        let name = if attempt == 1 {
            format!("{stem} ({}).{}", profile.label(), profile.extension())
        } else {
            format!(
                "{stem} ({} {attempt}).{}",
                profile.label(),
                profile.extension()
            )
        };
        let candidate = folder.join(name);
        if !candidate.exists() {
            return Ok(candidate);
        }
    }
    Err(CoreError::library(
        "conversion_output_exists",
        "That folder already holds 99 converted copies of this file. Choose another folder.",
    ))
}

fn same_file(left: &Path, right: &Path) -> bool {
    match (std::fs::canonicalize(left), std::fs::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => left == right,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filename::parse_filename;
    use crate::library::Ingest;

    fn library_with(names: &[&str]) -> (tempfile::TempDir, Library) {
        let directory = tempfile::tempdir().expect("tempdir");
        let library = Library::open_in_memory().expect("library");
        let source = library.add_source(directory.path(), None).expect("source");
        for name in names {
            std::fs::write(directory.path().join(name), b"0123456789").expect("write");
            let parsed = parse_filename(name, None);
            library
                .ingest_file(Ingest {
                    source_id: &source.id,
                    relative_path: name,
                    fingerprint: name,
                    size: 10,
                    modified_ms: 1_700_000_000_000,
                    scan_id: "scan-1",
                    parsed: Some(&parsed),
                    expected_root: Some(&source.root_path),
                })
                .expect("ingest");
        }
        (directory, library)
    }

    fn first_file(library: &Library) -> String {
        library
            .items(&crate::library::LibraryQuery::page(10))
            .expect("items")
            .first()
            .expect("one item")
            .id
            .clone()
    }

    #[test]
    fn a_plan_never_points_at_the_original_or_an_existing_output() {
        let (source_dir, library) = library_with(&["A.Film.2001.mkv"]);
        let file_id = first_file(&library);
        let output = tempfile::tempdir().expect("output");

        let plan = library
            .plan_conversion(&file_id, ConversionProfile::Remux, output.path())
            .expect("plan");
        assert_eq!(plan.profile, ConversionProfile::Remux);
        assert_eq!(
            plan.output_path
                .file_name()
                .map(|name| name.to_string_lossy().to_string()),
            Some("A.Film.2001 (remux).mp4".to_string())
        );
        assert_ne!(plan.source_path, plan.output_path);
        assert!(
            std::fs::canonicalize(&plan.source_path).is_ok_and(
                |path| path.starts_with(std::fs::canonicalize(source_dir.path()).unwrap())
            ),
            "the source must be the file in the granted folder"
        );
        assert_eq!(plan.source_size, 10);
        assert!(
            plan.source_modified_ms > 0,
            "the plan records the file's own modification time, not the scan's"
        );

        std::fs::write(&plan.output_path, b"taken").expect("occupy");
        let second = library
            .plan_conversion(&file_id, ConversionProfile::Remux, output.path())
            .expect("second plan");
        assert_ne!(second.output_path, plan.output_path);
        assert!(
            second
                .output_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("remux 2"))
        );

        let third = library
            .plan_conversion(&file_id, ConversionProfile::Standard, output.path())
            .expect("third plan");
        assert!(
            third
                .output_path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().contains("h264"))
        );
    }

    #[test]
    fn a_read_only_folder_is_refused_before_anything_is_written() {
        let (_source_dir, library) = library_with(&["A.Film.2001.mkv"]);
        let file_id = first_file(&library);
        let folder = tempfile::tempdir().expect("folder");
        let original = std::fs::metadata(folder.path())
            .expect("metadata")
            .permissions();
        let mut permissions = original.clone();
        permissions.set_readonly(true);
        std::fs::set_permissions(folder.path(), permissions).expect("readonly");

        let outcome = library.plan_conversion(
            &file_id,
            ConversionProfile::Remux,
            &folder.path().join("converted"),
        );
        let error = match outcome {
            Ok(_) => {
                let _ = std::fs::set_permissions(folder.path(), original);
                panic!("a folder that cannot be written to must be refused");
            }
            Err(error) => {
                let _ = std::fs::set_permissions(folder.path(), original);
                error
            }
        };
        assert_eq!(error.code(), "conversion_folder_invalid");
        assert!(!folder.path().join("converted").exists());
    }

    #[test]
    fn a_job_moves_forward_only_and_a_finished_one_is_not_reopened() {
        let (_source_dir, library) = library_with(&["A.Film.2001.mkv"]);
        let file_id = first_file(&library);
        let output = tempfile::tempdir().expect("output");

        assert!(
            library
                .plan_conversion("missing", ConversionProfile::Remux, output.path())
                .is_err()
        );

        let plan = library
            .plan_conversion(&file_id, ConversionProfile::Remux, output.path())
            .expect("plan");
        let job = library.begin_conversion(&plan).expect("job");
        assert_eq!(job.status, ConversionStatus::Queued);
        assert!(job.is_active());

        let duplicate = library.begin_conversion(&plan).expect_err("duplicate");
        assert_eq!(duplicate.code(), "conversion_in_progress");

        library
            .set_conversion_progress(&job.id, 40)
            .expect("progress");
        library
            .set_conversion_progress(&job.id, 10)
            .expect("progress");
        let recorded = library
            .conversion_for_file(&file_id)
            .expect("lookup")
            .expect("job");
        assert_eq!(recorded.progress_percent, 40);
        assert_eq!(recorded.status, ConversionStatus::Running);

        library
            .finish_conversion(&job.id, ConversionStatus::Done, "checked", 900)
            .expect("finish");
        let done = library
            .conversion_for_file(&file_id)
            .expect("lookup")
            .expect("job");
        assert_eq!(done.status, ConversionStatus::Done);
        assert_eq!(done.progress_percent, 100);
        assert_eq!(done.output_bytes, 900);
        assert!(!library.cancel_conversion(&job.id).expect("cancel"));
        assert!(
            library
                .set_conversion_status(&job.id, ConversionStatus::Running)
                .is_err()
        );
        assert!(library.active_conversion().expect("active").is_none());
    }

    #[test]
    fn a_running_job_can_be_stopped_and_says_so() {
        let (_source_dir, library) = library_with(&["A.Film.2001.mkv"]);
        let file_id = first_file(&library);
        let output = tempfile::tempdir().expect("output");
        let plan = library
            .plan_conversion(&file_id, ConversionProfile::AudioFix, output.path())
            .expect("plan");
        let job = library.begin_conversion(&plan).expect("job");
        library
            .set_conversion_progress(&job.id, 55)
            .expect("progress");

        assert!(library.cancel_conversion(&job.id).expect("cancel"));
        let stopped = library
            .conversion_for_file(&file_id)
            .expect("lookup")
            .expect("job");
        assert_eq!(stopped.status, ConversionStatus::Cancelled);
        assert!(stopped.message.contains("Stopped"));

        let plan_again = library
            .plan_conversion(&file_id, ConversionProfile::AudioFix, output.path())
            .expect("plan again");
        assert!(
            library.begin_conversion(&plan_again).is_ok(),
            "a stopped job must not block a fresh attempt"
        );
    }

    #[test]
    fn a_result_is_checked_against_the_original_before_it_counts() {
        let good = OutputFacts {
            duration_ms: 6_000_000,
            has_video: true,
            has_audio: true,
        };
        assert!(verify_output(6_000_000, true, &good).is_ok());
        assert!(verify_output(6_030_000, true, &good).is_ok());
        assert!(verify_output(0, true, &good).is_ok());

        let short = OutputFacts {
            duration_ms: 5_000_000,
            ..good
        };
        assert_eq!(
            verify_output(6_000_000, true, &short).unwrap_err().code(),
            "conversion_verify_failed"
        );
        let silent = OutputFacts {
            has_audio: false,
            ..good
        };
        assert_eq!(
            verify_output(6_000_000, true, &silent).unwrap_err().code(),
            "conversion_verify_failed"
        );
        assert!(
            verify_output(6_000_000, false, &silent).is_ok(),
            "a source with no audio cannot lose it"
        );
        let mute = OutputFacts {
            has_video: false,
            ..good
        };
        assert_eq!(
            verify_output(6_000_000, true, &mute).unwrap_err().code(),
            "conversion_verify_failed"
        );
        let unknown = OutputFacts {
            duration_ms: 0,
            ..good
        };
        assert_eq!(
            verify_output(6_000_000, true, &unknown).unwrap_err().code(),
            "conversion_verify_failed"
        );
    }

    #[test]
    fn profile_ids_round_trip_and_read_back_in_a_fresh_library() {
        for profile in ConversionProfile::ALL {
            assert_eq!(ConversionProfile::from_id(profile.id()), Some(profile));
            assert!(!profile.summary().trim().is_empty());
        }
        assert_eq!(ConversionProfile::from_id("guess"), None);
        assert_eq!(ConversionStatus::parse("nothing"), ConversionStatus::Queued);

        let (_source_dir, library) = library_with(&["A.Film.2001.mkv"]);
        let file_id = first_file(&library);
        let output = tempfile::tempdir().expect("output");
        let plan = library
            .plan_conversion(&file_id, ConversionProfile::Standard, output.path())
            .expect("plan");
        library.begin_conversion(&plan).expect("job");

        let recent = library.recent_conversions(5).expect("recent");
        assert_eq!(recent.len(), 1);
        assert_eq!(recent[0].profile, ConversionProfile::Standard);
        assert!(recent[0].summary().contains("h264"));
        assert_eq!(
            library.recent_conversions(0).expect("clamped").len(),
            1,
            "a zero limit is clamped instead of returning everything"
        );
    }
}
