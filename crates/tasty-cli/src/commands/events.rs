//! `tasty events ...` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum EventsCommands {
    /// Read events from a position. Prints one JSON object per line.
    ///
    /// The cursor is yours, not the host's: pass the `next_offset` from the
    /// previous answer to continue. Asking twice with the same arguments gives
    /// the same answer, and a slow reader queues nothing on the host side.
    ///
    /// Events live in memory only. A restart clears them and starts positions
    /// over, which is why every answer carries an `epoch` — if it differs from
    /// the one your offset came from, that offset is from a previous generation.
    Fetch {
        /// Position to read from. Default 0, the oldest still retained.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// How many events at most. Clamped by the host.
        #[arg(long)]
        max: Option<u64>,
        /// Key pattern: an exact key (`agent.task_finished`) or a namespace
        /// wildcard (`agent.*`). Same grammar the subscriptions use.
        #[arg(long)]
        filter: Option<String>,
        /// Wait up to this long for something to arrive before answering.
        /// Default 0, answer immediately with whatever is there.
        #[arg(long)]
        wait_ms: Option<u64>,
    },
    /// Follow the feed: long-poll from a position and print each event as a
    /// line of JSON, forever. Runs until interrupted.
    ///
    /// Reconnecting is the same call with the last position, so a consumer that
    /// was stopped resumes where it left off. If it was stopped long enough for
    /// its position to scroll out of the ring, the answer says so instead of
    /// quietly restarting from the oldest event, and this command prints that
    /// notice to stderr so a `while read` loop on stdout is not disturbed.
    Follow {
        /// Position to start from. Default 0, the oldest still retained.
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// Key pattern, same grammar as `fetch --filter`.
        #[arg(long)]
        filter: Option<String>,
        /// How many events per request.
        #[arg(long, default_value_t = 256)]
        batch: u64,
        /// How long each request waits before coming back empty.
        #[arg(long, default_value_t = 30_000)]
        wait_ms: u64,
    },
}
