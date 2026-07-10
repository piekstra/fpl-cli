use clap::{Parser, Subcommand};

/// Manage your Florida Power & Light (FPL) account from the command line.
///
/// FPL publishes no official API; this talks to the same www.fpl.com JSON
/// services the website and mobile app use. Your password lives in the OS
/// keychain (with a `$FPL_PASSWORD` env fallback) and never touches disk.
///
/// Work in progress: the outage feed needs no login and is fully verified.
/// Authenticated commands target endpoints mapped from FPL's own web app; if a
/// response shape differs for your account, use `--json` (or `fpl api`) to see
/// the raw payload and please open an issue.
#[derive(Parser, Debug)]
#[command(name = "fpl", version, about, long_about = None)]
pub struct Cli {
    /// Emit machine-readable JSON on stdout (diagnostics go to stderr).
    #[arg(long, global = true)]
    pub json: bool,

    /// Extra diagnostics on stderr (never secrets).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error stderr output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color (reserved; output is currently plain).
    #[arg(long, global = true)]
    pub no_color: bool,

    /// FPL login (email). Falls back to config, then $FPL_USERNAME, then prompts.
    #[arg(long, global = true, env = "FPL_USERNAME")]
    pub username: Option<String>,

    /// FPL account number. Falls back to config, then the first account on the
    /// login, if any.
    #[arg(long, global = true, env = "FPL_ACCOUNT")]
    pub account: Option<String>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Store your FPL credentials in the OS keychain and verify them.
    ///
    /// Prompts (no echo) for the password unless `$FPL_PASSWORD` is set. The
    /// username is remembered in config; the password only in the keychain.
    Login {
        /// Skip the live login check; just store the credentials.
        #[arg(long)]
        no_verify: bool,
    },

    /// Remove stored credentials from the keychain (and end the FPL session).
    Logout {
        /// Also delete the saved username/account from config.
        #[arg(long)]
        forget: bool,
    },

    /// Show whether credentials are configured (never reveals the password).
    Status,

    /// List the accounts on your FPL login.
    Accounts,

    /// Show account details: service address, meter, bill cycle, programs.
    Account,

    /// Show the current balance and due date.
    Balance,

    /// Billing: current/projected bill, history, budget billing, PDF download.
    #[command(subcommand)]
    Bill(BillCommand),

    /// Payments: methods, history, and making a payment.
    #[command(subcommand)]
    Pay(PayCommand),

    /// Energy usage: current summary, daily/hourly detail, appliance breakdown.
    #[command(subcommand)]
    Usage(UsageCommand),

    /// Account ledgers: transaction, deposit, and document history.
    #[command(subcommand)]
    History(HistoryCommand),

    /// Public power-outage counts (no login required).
    Outages {
        /// Filter to counties whose name contains this text (case-insensitive).
        #[arg(long)]
        county: Option<String>,
    },

    /// Raw authenticated request to any FPL endpoint (escape hatch).
    ///
    /// Example: `fpl api GET /cs/customer/v1/resources/header`
    Api {
        /// HTTP method: GET, POST, PUT, or DELETE.
        method: String,
        /// Path (leading slash, relative to https://www.fpl.com) or full URL.
        path: String,
        /// Request body as a JSON string (for POST/PUT).
        #[arg(long)]
        data: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BillCommand {
    /// Current period: projected bill, bill-to-date, daily average.
    Current,
    /// Prior bills (amounts, due dates, usage).
    History,
    /// Projected end-of-cycle bill for the current period.
    Projected,
    /// Budget Billing plan status and monthly graph.
    Budget,
    /// Download your latest bill PDF.
    Download {
        /// Output file path (default: ./fpl-bill.pdf).
        #[arg(short, long, default_value = "fpl-bill.pdf")]
        out: String,
    },
}

#[derive(Subcommand, Debug)]
pub enum PayCommand {
    /// List saved payment methods / options.
    Methods,
    /// Payment history (from the account ledger).
    History,
    /// Make a payment. Prompts for confirmation unless `--yes` is given.
    Make {
        /// Amount in dollars, e.g. 123.45.
        #[arg(long)]
        amount: String,
        /// Payment date as MM-DD-YYYY (default: today).
        #[arg(long)]
        date: Option<String>,
        /// Payment method / bank-account token id (from `fpl pay methods`).
        #[arg(long)]
        method: Option<String>,
        /// Skip the interactive confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum UsageCommand {
    /// Current-period energy summary (projected kWh, daily average, cost).
    Summary,
    /// Hourly usage for a single day.
    Hourly {
        /// Day as MM-DD-YYYY (default: yesterday).
        #[arg(long)]
        date: Option<String>,
    },
    /// Appliance-level (disaggregated) usage for the latest bill period.
    Appliances,
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    /// Transaction / account history (charges, payments, adjustments).
    Account,
    /// Deposit history.
    Deposit,
    /// Document history (bills, notices available to download).
    Documents,
}
