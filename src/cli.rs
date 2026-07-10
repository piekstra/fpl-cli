use clap::{Args, Parser, Subcommand};

/// Manage your Florida Power & Light (FPL) account from the command line.
///
/// FPL publishes no official API; this talks to the same www.fpl.com JSON
/// services the website and mobile app use. Your password lives only in the OS
/// keychain — set it up with `fpl init` (or `fpl set-credential`), which read
/// the secret from stdin or an env var, never a flag.
///
/// Output is human- and agent-friendly text by default; resource reads render
/// key/value blocks and pipe-delimited tables. For a raw JSON payload (handy
/// while FPL's response shapes are still being mapped), use `fpl api`.
#[derive(Parser, Debug)]
#[command(name = "fpl", version, about, long_about = None)]
pub struct Cli {
    /// Account number to act on. Overrides the active account and $FPL_ACCOUNT.
    #[arg(short = 'a', long, global = true, env = "FPL_ACCOUNT")]
    pub account: Option<String>,

    /// FPL login (email). Falls back to config, then $FPL_USERNAME.
    #[arg(long, global = true, env = "FPL_USERNAME")]
    pub username: Option<String>,

    /// Extra diagnostics on stderr (never secrets).
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Suppress non-error stderr output.
    #[arg(short, long, global = true)]
    pub quiet: bool,

    /// Disable ANSI color (reserved; output is currently plain).
    #[arg(long, global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// First-time setup: verify credentials and store them in the keychain.
    ///
    /// Flag-driven and fully scriptable. Supply the password via
    /// `--password-stdin` or `--password-from-env`; there is no password flag.
    Init(InitArgs),

    /// Write a single credential to the keychain (rotation / headless setup).
    ///
    /// Reads the secret from `--stdin` or `--from-env <VAR>` (exactly one).
    /// Refuses to replace an existing entry unless `--overwrite` is given.
    SetCredential(SetCredentialArgs),

    /// Session and credential status.
    #[command(subcommand)]
    Auth(AuthCommand),

    /// Accounts on your login: list, detail, active-account selection, balance.
    #[command(subcommand)]
    Accounts(AccountsCommand),

    /// Billing: current/projected bill, history, budget billing, PDF download.
    #[command(subcommand)]
    Bills(BillsCommand),

    /// Payments: history, saved methods, and making a payment.
    #[command(subcommand)]
    Payments(PaymentsCommand),

    /// Energy usage: current summary, hourly detail, appliance breakdown.
    #[command(subcommand)]
    Usage(UsageCommand),

    /// Account ledgers: transaction and deposit history.
    #[command(subcommand)]
    History(HistoryCommand),

    /// Public power-outage counts (no login required).
    #[command(subcommand)]
    Outages(OutagesCommand),

    /// Raw authenticated request to any FPL endpoint (returns JSON).
    ///
    /// Example: `fpl api GET /cs/customer/v1/resources/header`
    Api(ApiArgs),
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// Read the password from stdin (one line).
    #[arg(long)]
    pub password_stdin: bool,
    /// Read the password from a named environment variable.
    #[arg(long, value_name = "VAR")]
    pub password_from_env: Option<String>,
    /// Store the credentials without a live login check.
    #[arg(long)]
    pub no_verify: bool,
    /// Replace an existing stored password instead of failing.
    #[arg(long)]
    pub overwrite: bool,
    /// Never prompt; fail if a required input is missing.
    #[arg(long)]
    pub non_interactive: bool,
    /// Emit the result as JSON on stdout (confirmation still goes to stderr).
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct SetCredentialArgs {
    /// Read the secret from stdin (one line).
    #[arg(long)]
    pub stdin: bool,
    /// Read the secret from a named environment variable.
    #[arg(long, value_name = "VAR")]
    pub from_env: Option<String>,
    /// Replace an existing entry instead of failing.
    #[arg(long)]
    pub overwrite: bool,
    /// Emit the result as JSON on stdout (confirmation still goes to stderr).
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Show configured username, active account, and keychain state.
    Status {
        /// Emit as JSON.
        #[arg(long)]
        json: bool,
    },
    /// End the FPL session and remove the stored password.
    Logout {
        /// Also clear the saved username/account from config.
        #[arg(long)]
        forget: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum AccountsCommand {
    /// List the accounts on your login.
    #[command(alias = "ls")]
    List,
    /// Show details for one account (defaults to the active account).
    Get {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Select the active account for subsequent commands.
    Use {
        /// Account number to make active.
        account_id: String,
    },
    /// Show the current balance and due date.
    Balance {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum BillsCommand {
    /// Prior bills (amounts, due dates, usage).
    #[command(alias = "ls")]
    List {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Current period: projected bill, bill-to-date, daily average.
    Get {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Projected end-of-cycle bill for the current period.
    Projected {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Budget Billing plan status and monthly graph.
    Budget {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum PaymentsCommand {
    /// Payment history (from the account ledger).
    #[command(alias = "ls")]
    List {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// List saved payment methods / options.
    Methods {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Make a payment. Prompts for confirmation unless `--force` is given.
    Create {
        /// Amount in dollars, e.g. 123.45.
        #[arg(long)]
        amount: String,
        /// Payment date as MM-DD-YYYY (default: today).
        #[arg(long)]
        date: Option<String>,
        /// Payment method / bank-account token id (from `payments methods`).
        #[arg(long)]
        method: Option<String>,
        /// Account number (defaults to active / --account).
        #[arg(long)]
        account_id: Option<String>,
        /// Skip the confirmation prompt (submits the payment).
        #[arg(long)]
        force: bool,
    },
}

#[derive(Subcommand, Debug)]
pub enum UsageCommand {
    /// Current-period energy summary (projected kWh, daily average, cost).
    Get {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
    /// Hourly usage for a single day.
    Hourly {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
        /// Day as MM-DD-YYYY (default: yesterday).
        #[arg(long)]
        date: Option<String>,
    },
    /// Appliance-level (disaggregated) usage for the latest bill period.
    Appliances {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
pub enum HistoryCommand {
    /// List ledger entries of a given kind.
    #[command(alias = "ls")]
    List {
        /// Account number (defaults to active / --account).
        account_id: Option<String>,
        /// Which ledger: account or deposit.
        #[arg(long, default_value = "account")]
        r#type: String,
    },
    /// List the valid `--type` values for `history list`.
    Types,
}

#[derive(Subcommand, Debug)]
pub enum OutagesCommand {
    /// County-level outage counts.
    #[command(alias = "ls")]
    List {
        /// Filter to counties whose name contains this text (case-insensitive).
        #[arg(long)]
        name: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct ApiArgs {
    /// HTTP method: GET, POST, PUT, or DELETE.
    pub method: String,
    /// Path (leading slash, relative to https://www.fpl.com) or full URL.
    pub path: String,
    /// Request body as a JSON string (for POST/PUT).
    #[arg(long)]
    pub data: Option<String>,
}
