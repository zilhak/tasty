//! `tasty events ...` subcommand 정의.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum EventsCommands {
    /// Read events from a position. Prints the answer as one JSON object.
    ///
    /// The events are in its `events` array, next to `next_offset`, `epoch`
    /// and the other fields of the answer. For one event per line, use
    /// `follow`.
    ///
    /// Pass next_offset from the previous response to continue. Reads do not
    /// advance a host-side cursor or queue events for slow readers. Repeating a
    /// read may return different data as the retained feed changes.
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
    ///
    /// A restart of tasty starts positions over. Pass the `--epoch` your offset
    /// came from when you reattach: if tasty restarted since, this command says
    /// so on stderr and starts from the beginning of the new generation. Without
    /// it, a position past the end of the new feed is reported and handled the
    /// same way. If the connection drops, this command prints the `--offset` and
    /// `--epoch` to reattach with and exits; with `--reconnect` it keeps trying
    /// instead and carries on.
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
        /// Generation your offset came from, as printed when the connection
        /// drops or returned by `fetch`. If tasty restarted since, start over
        /// from the new generation instead of reading an unrelated position.
        #[arg(long)]
        epoch: Option<u64>,
        /// When the connection drops, retry every second instead of exiting.
        /// The generation is carried over, so a restart is reported and
        /// followed from its start.
        #[arg(long)]
        reconnect: bool,
    },
}
