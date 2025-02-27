//! ## FileTransferActivity
//!
//! `filetransfer_activiy` is the module which implements the Filetransfer activity, which is the main activity afterall

// This module is split into files, cause it's just too big
mod actions;
mod components;
mod fswatcher;
mod lib;
mod misc;
mod session;
mod update;
mod view;

// locals
use super::{Activity, Context, ExitReason};
use crate::config::themes::Theme;
use crate::explorer::{FileExplorer, FileSorting};
use crate::filetransfer::{Builder, FileTransferParams};
use crate::host::Localhost;
use crate::system::config_client::ConfigClient;
use crate::system::watcher::FsWatcher;
pub(self) use lib::browser;
use lib::browser::Browser;
use lib::transfer::{TransferOpts, TransferStates};
pub(self) use session::TransferPayload;

// Includes
use chrono::{DateTime, Local};
use remotefs::RemoteFs;
use std::collections::VecDeque;
use std::time::Duration;
use tempfile::TempDir;
use tuirealm::{Application, EventListenerCfg, NoUserEvent};

// -- components

#[derive(Debug, Eq, PartialEq, Clone, Hash)]
enum Id {
    CopyPopup,
    DeletePopup,
    DisconnectPopup,
    ErrorPopup,
    ExecPopup,
    ExplorerFind,
    ExplorerLocal,
    ExplorerRemote,
    FatalPopup,
    FileInfoPopup,
    FindPopup,
    FooterBar,
    GlobalListener,
    GotoPopup,
    KeybindingsPopup,
    Log,
    MkdirPopup,
    NewfilePopup,
    OpenWithPopup,
    ProgressBarFull,
    ProgressBarPartial,
    QuitPopup,
    RenamePopup,
    ReplacePopup,
    ReplacingFilesListPopup,
    SaveAsPopup,
    SortingPopup,
    StatusBarLocal,
    StatusBarRemote,
    SymlinkPopup,
    SyncBrowsingMkdirPopup,
    WaitPopup,
    WatchedPathsList,
    WatcherPopup,
}

#[derive(Debug, PartialEq)]
enum Msg {
    PendingAction(PendingActionMsg),
    Transfer(TransferMsg),
    Ui(UiMsg),
    None,
}

#[derive(Debug, PartialEq)]
enum PendingActionMsg {
    CloseReplacePopups,
    CloseSyncBrowsingMkdirPopup,
    MakePendingDirectory,
    TransferPendingFile,
}

#[derive(Debug, PartialEq)]
enum TransferMsg {
    AbortTransfer,
    CopyFileTo(String),
    CreateSymlink(String),
    DeleteFile,
    EnterDirectory,
    ExecuteCmd(String),
    GoTo(String),
    GoToParentDirectory,
    GoToPreviousDirectory,
    Mkdir(String),
    NewFile(String),
    OpenFile,
    OpenFileWith(String),
    OpenTextFile,
    ReloadDir,
    RenameFile(String),
    SaveFileAs(String),
    SearchFile(String),
    ToggleWatch,
    ToggleWatchFor(usize),
    TransferFile,
}

#[derive(Debug, PartialEq)]
enum UiMsg {
    ChangeFileSorting(FileSorting),
    ChangeTransferWindow,
    CloseCopyPopup,
    CloseDeletePopup,
    CloseDisconnectPopup,
    CloseErrorPopup,
    CloseExecPopup,
    CloseFatalPopup,
    CloseFileInfoPopup,
    CloseFileSortingPopup,
    CloseFindExplorer,
    CloseFindPopup,
    CloseGotoPopup,
    CloseKeybindingsPopup,
    CloseMkdirPopup,
    CloseNewFilePopup,
    CloseOpenWithPopup,
    CloseQuitPopup,
    CloseRenamePopup,
    CloseSaveAsPopup,
    CloseSymlinkPopup,
    CloseWatchedPathsList,
    CloseWatcherPopup,
    Disconnect,
    LogBackTabbed,
    Quit,
    ReplacePopupTabbed,
    ShowCopyPopup,
    ShowDeletePopup,
    ShowDisconnectPopup,
    ShowExecPopup,
    ShowFileInfoPopup,
    ShowFileSortingPopup,
    ShowFindPopup,
    ShowGotoPopup,
    ShowKeybindingsPopup,
    ShowLogPanel,
    ShowMkdirPopup,
    ShowNewFilePopup,
    ShowOpenWithPopup,
    ShowQuitPopup,
    ShowRenamePopup,
    ShowSaveAsPopup,
    ShowSymlinkPopup,
    ShowWatchedPathsList,
    ShowWatcherPopup,
    ToggleHiddenFiles,
    ToggleSyncBrowsing,
    WindowResized,
}

/// Log level type
enum LogLevel {
    Error,
    Warn,
    Info,
}

/// Log record entry
struct LogRecord {
    pub time: DateTime<Local>,
    pub level: LogLevel,
    pub msg: String,
}

impl LogRecord {
    /// Instantiates a new LogRecord
    pub fn new(level: LogLevel, msg: String) -> LogRecord {
        LogRecord {
            time: Local::now(),
            level,
            msg,
        }
    }
}

/// FileTransferActivity is the data holder for the file transfer activity
pub struct FileTransferActivity {
    /// Exit reason
    exit_reason: Option<ExitReason>,
    /// Context holder
    context: Option<Context>,
    /// Tui-realm application
    app: Application<Id, Msg, NoUserEvent>,
    /// Whether should redraw UI
    redraw: bool,
    /// Localhost bridge
    host: Localhost,
    /// Remote host client
    client: Box<dyn RemoteFs>,
    /// Browser
    browser: Browser,
    /// Current log lines
    log_records: VecDeque<LogRecord>,
    transfer: TransferStates,
    /// Temporary directory where to store temporary stuff
    cache: Option<TempDir>,
    /// Fs watcher
    fswatcher: Option<FsWatcher>,
}

impl FileTransferActivity {
    /// Instantiates a new FileTransferActivity
    pub fn new(host: Localhost, params: &FileTransferParams, ticks: Duration) -> Self {
        // Get config client
        let config_client: ConfigClient = Self::init_config_client();
        Self {
            exit_reason: None,
            context: None,
            app: Application::init(
                EventListenerCfg::default()
                    .poll_timeout(ticks)
                    .default_input_listener(ticks),
            ),
            redraw: true,
            host,
            client: Builder::build(params.protocol, params.params.clone(), &config_client),
            browser: Browser::new(&config_client),
            log_records: VecDeque::with_capacity(256), // 256 events is enough I guess
            transfer: TransferStates::default(),
            cache: match TempDir::new() {
                Ok(d) => Some(d),
                Err(_) => None,
            },
            fswatcher: match FsWatcher::init(Duration::from_secs(5)) {
                Ok(w) => Some(w),
                Err(e) => {
                    error!("failed to initialize fs watcher: {}", e);
                    None
                }
            },
        }
    }

    fn local(&self) -> &FileExplorer {
        self.browser.local()
    }

    fn local_mut(&mut self) -> &mut FileExplorer {
        self.browser.local_mut()
    }

    fn remote(&self) -> &FileExplorer {
        self.browser.remote()
    }

    fn remote_mut(&mut self) -> &mut FileExplorer {
        self.browser.remote_mut()
    }

    fn found(&self) -> Option<&FileExplorer> {
        self.browser.found()
    }

    fn found_mut(&mut self) -> Option<&mut FileExplorer> {
        self.browser.found_mut()
    }

    /// Get file name for a file in cache
    fn get_cache_tmp_name(&self, name: &str, file_type: Option<&str>) -> Option<String> {
        self.cache.as_ref().map(|_| {
            let base: String = format!(
                "{}-{}",
                name,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            );
            match file_type {
                None => base,
                Some(file_type) => format!("{}.{}", base, file_type),
            }
        })
    }

    /// Returns a reference to context
    fn context(&self) -> &Context {
        self.context.as_ref().unwrap()
    }

    /// Returns a mutable reference to context
    fn context_mut(&mut self) -> &mut Context {
        self.context.as_mut().unwrap()
    }

    /// Returns config client reference
    fn config(&self) -> &ConfigClient {
        self.context().config()
    }

    /// Get a reference to `Theme`
    fn theme(&self) -> &Theme {
        self.context().theme_provider().theme()
    }

    /// Map a function to fs watcher if any
    fn map_on_fswatcher<F, T>(&mut self, mapper: F) -> Option<T>
    where
        F: FnOnce(&mut FsWatcher) -> T,
    {
        self.fswatcher.as_mut().map(mapper)
    }
}

/**
 * Activity Trait
 * Keep it clean :)
 * Use methods instead!
 */

impl Activity for FileTransferActivity {
    /// `on_create` is the function which must be called to initialize the activity.
    /// `on_create` must initialize all the data structures used by the activity
    fn on_create(&mut self, context: Context) {
        debug!("Initializing activity...");
        // Set context
        self.context = Some(context);
        // Clear terminal
        if let Err(err) = self.context.as_mut().unwrap().terminal().clear_screen() {
            error!("Failed to clear screen: {}", err);
        }
        // Put raw mode on enabled
        if let Err(err) = self.context_mut().terminal().enable_raw_mode() {
            error!("Failed to enter raw mode: {}", err);
        }
        // Get files at current pwd
        self.reload_local_dir();
        debug!("Read working directory");
        // Configure text editor
        self.setup_text_editor();
        debug!("Setup text editor");
        // init view
        self.init();
        debug!("Initialized view");
        // Verify error state from context
        if let Some(err) = self.context.as_mut().unwrap().error() {
            error!("Fatal error on create: {}", err);
            self.mount_fatal(&err);
        }
        info!("Created FileTransferActivity");
    }

    /// `on_draw` is the function which draws the graphical interface.
    /// This function must be called at each tick to refresh the interface
    fn on_draw(&mut self) {
        // Context must be something
        if self.context.is_none() {
            return;
        }
        // Check if connected (popup must be None, otherwise would try reconnecting in loop in case of error)
        if !self.client.is_connected() && !self.app.mounted(&Id::FatalPopup) {
            let ftparams = self.context().ft_params().unwrap();
            // print params
            let msg: String = Self::get_connection_msg(&ftparams.params);
            // Set init state to connecting popup
            self.mount_wait(msg.as_str());
            // Force ui draw
            self.view();
            // Connect to remote
            self.connect();
            // Redraw
            self.redraw = true;
        }
        self.tick();
        // poll
        self.poll_watcher();
        // View
        if self.redraw {
            self.view();
        }
    }

    /// `will_umount` is the method which must be able to report to the activity manager, whether
    /// the activity should be terminated or not.
    /// If not, the call will return `None`, otherwise return`Some(ExitReason)`
    fn will_umount(&self) -> Option<&ExitReason> {
        self.exit_reason.as_ref()
    }

    /// `on_destroy` is the function which cleans up runtime variables and data before terminating the activity.
    /// This function must be called once before terminating the activity.
    fn on_destroy(&mut self) -> Option<Context> {
        // Destroy cache
        if let Some(cache) = self.cache.take() {
            if let Err(err) = cache.close() {
                error!("Failed to delete cache: {}", err);
            }
        }
        // Disable raw mode
        if let Err(err) = self.context_mut().terminal().disable_raw_mode() {
            error!("Failed to disable raw mode: {}", err);
        }
        if let Err(err) = self.context_mut().terminal().clear_screen() {
            error!("Failed to clear screen: {}", err);
        }
        // Disconnect client
        if self.client.is_connected() {
            let _ = self.client.disconnect();
        }
        self.context.take()
    }
}
