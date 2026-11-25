//! Optional ad-hoc reverse-engineering probes and corpus experiments (`--features adhoc`).
//!
//! Each probe lives in its own submodule here and is exposed under `musicdb adhoc <probe>` so
//! general-purpose user builds (without `--features adhoc`) do not compile them and individual
//! probes can be run without re-running unrelated corpus scans.

use std::path::PathBuf;

use anyhow::Result;
use clap::Subcommand;

mod artwork_states;
mod codecs;
pub(crate) mod corpus_audit;
mod experiments_2026_11_21;
mod experiments_2026_11_23;
mod helpers;
mod history_shaky;
mod local_checks_2026_11_21;
mod lyrics;
mod media_probes;
mod offset_116;
mod plma_raw;
mod static_corr_2026_11_25;

#[derive(Subcommand)]
pub(crate) enum AdhocCommand {
    /// Run corpus verification across .musicdb snapshots and optional iTunes XML
    CorpusAudit {
        /// Directories or .musicdb files to scan recursively
        #[arg(required = true)]
        paths: Vec<PathBuf>,
        /// Optional path to iTunes Music Library.xml for timestamp verification
        #[arg(long)]
        xml: Option<PathBuf>,
        /// Optional pair of snapshots across a timezone change (`A` then `B`)
        #[arg(long, num_args = 2)]
        tz_pair: Option<Vec<PathBuf>>,
    },
    /// Probe `TrackNumerics[59]` (catalog lyrics state) vs `boma 0x0036` plist keys
    Lyrics {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Probe `TrackNumerics` `subrecord_kind` 1 vs 3 codec and container mappings
    Codecs {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Probe `TrackNumerics[116..120]` timestamp distribution across corpus files
    #[command(name = "offset-116")]
    Offset116 {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Probe artwork cache state bytes against `artwork.sqlite`
    ArtworkStates {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Dump `LibraryMasterHeader` (`plma`) 194-byte layout and alignment across libraries
    PlmaRaw {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Verify `history.md` Section 5 shaky fields and bitfields across snapshots
    HistoryShaky {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Cross-check `ffprobe`/`exiftool`/`.movpkg` discoveries across the corpus
    MediaProbes {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Analyze 2026-11-21 City Life A/B offline import and Stylo `.m4v` video snapshots
    #[command(name = "experiments-2026-11-21")]
    Experiments20261121 {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Analyze 2026-11-23 offline-local-probes campaign (`City Life` A/B playback, `itma[54]`, local video/ALAC/WAV/AIFF)
    #[command(name = "experiments-2026-11-23")]
    Experiments20261123 {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Run 2026-11-21 local corpus checks (`itma[53]`, `TrackPlaysSkips::unk_40`, `itma[33]`, `iama[128..136]`, `plma[182..186]`, `ipfa[44..52]`, `lpma[192]`, `SmartPlaylistOptions[16]`)
    #[command(name = "local-checks-2026-11-21")]
    LocalChecks20261121 {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Run 2026-11-25 static/historical correlation pass on verified captures
    #[command(name = "static-corr-2026-11-25")]
    StaticCorr20261125 {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
    /// Run every historical corpus probe sequentially
    AllProbes {
        /// Corpus directory to inspect
        corpus_dir: PathBuf,
    },
}

pub(crate) fn run(command: AdhocCommand) -> Result<()> {
    match command {
        AdhocCommand::CorpusAudit {
            paths,
            xml,
            tz_pair,
        } => {
            let pair = tz_pair
                .as_deref()
                .and_then(|v| Some((v.first()?.as_path(), v.get(1)?.as_path())));
            let report = corpus_audit::run_corpus_audit(&paths, xml.as_deref(), pair)?;
            print!("{report}");
            Ok(())
        }
        AdhocCommand::Lyrics { corpus_dir } => lyrics::run(&corpus_dir),
        AdhocCommand::Codecs { corpus_dir } => codecs::run(&corpus_dir),
        AdhocCommand::Offset116 { corpus_dir } => offset_116::run(&corpus_dir),
        AdhocCommand::ArtworkStates { corpus_dir } => artwork_states::run(&corpus_dir),
        AdhocCommand::PlmaRaw { corpus_dir } => plma_raw::run(&corpus_dir),
        AdhocCommand::HistoryShaky { corpus_dir } => history_shaky::run(&corpus_dir),
        AdhocCommand::MediaProbes { corpus_dir } => media_probes::run(&corpus_dir),
        AdhocCommand::Experiments20261121 { corpus_dir } => {
            experiments_2026_11_21::run(&corpus_dir)
        }
        AdhocCommand::Experiments20261123 { corpus_dir } => {
            experiments_2026_11_23::run(&corpus_dir)
        }
        AdhocCommand::LocalChecks20261121 { corpus_dir } => {
            local_checks_2026_11_21::run(&corpus_dir)
        }
        AdhocCommand::StaticCorr20261125 { corpus_dir } => static_corr_2026_11_25::run(&corpus_dir),
        AdhocCommand::AllProbes { corpus_dir } => {
            lyrics::run(&corpus_dir)?;
            println!();
            codecs::run(&corpus_dir)?;
            println!();
            offset_116::run(&corpus_dir)?;
            println!();
            artwork_states::run(&corpus_dir)?;
            println!();
            plma_raw::run(&corpus_dir)?;
            println!();
            history_shaky::run(&corpus_dir)?;
            println!();
            media_probes::run(&corpus_dir)?;
            println!();
            experiments_2026_11_21::run(&corpus_dir)?;
            println!();
            local_checks_2026_11_21::run(&corpus_dir)?;
            println!();
            experiments_2026_11_23::run(&corpus_dir)
        }
    }
}
