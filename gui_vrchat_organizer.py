#!/usr/bin/env python3
"""Simple Tkinter GUI for VRChat Organizer"""
import threading
import json
import queue
import logging
import sys
import os
import subprocess
from pathlib import Path # Keep Path for file operations
import tkinter as tk # Keep tk for root, BooleanVar, StringVar, IntVar, messagebox, scrolledtext
from tkinter import filedialog, scrolledtext, messagebox, font, ttk # Add ttk

from organize_vrchat import VRChatOrganizer

# Enable High DPI scaling on Windows
if sys.platform.startswith('win'):
    try:
        from ctypes import windll
        windll.shcore.SetProcessDpiAwareness(1)
    except Exception:
        pass

# Configure logging for GUI
logger = logging.getLogger('vrchat_gui')
logger.setLevel(logging.INFO)

class TextHandler(logging.Handler):
    def __init__(self, log_queue):
        logging.Handler.__init__(self)
        self.log_queue = log_queue

    def emit(self, record):
        try:
            msg = self.format(record)
            self.log_queue.put(msg)
        except Exception:
            self.handleError(record)

class ToolTip:
    """Simple ToolTip implementation for Tkinter widgets."""
    def __init__(self, widget, text):
        self.widget = widget
        self.text = text
        self.tip_window = None
        self.id = None
        self.x = self.y = 0
        self.widget.bind("<Enter>", self.enter)
        self.widget.bind("<Leave>", self.leave)

    def enter(self, event=None):
        self.schedule()

    def leave(self, event=None):
        self.unschedule()
        self.hide_tip()

    def schedule(self):
        self.unschedule()
        self.id = self.widget.after(500, self.show_tip)

    def unschedule(self):
        id_ = self.id
        self.id = None
        if id_:
            self.widget.after_cancel(id_)

    def show_tip(self, event=None):
        x = self.widget.winfo_rootx() + 20
        y = self.widget.winfo_rooty() + self.widget.winfo_height() + 10
        self.tip_window = tw = tk.Toplevel(self.widget)
        tw.wm_overrideredirect(True)
        tw.wm_geometry(f"+{x}+{y}")
        label = tk.Label(tw, text=self.text, justify=tk.LEFT,
                      background="#ffffe0", relief=tk.SOLID, borderwidth=1,
                      font=("tahoma", "9", "normal"))
        label.pack(ipadx=1)

    def hide_tip(self):
        tw = self.tip_window
        self.tip_window = None
        if tw:
            tw.destroy()

class App:
    def __init__(self, root):
        self.root = root
        root.title('VRChat Organizer')

        # visual tweaks
        # Use ttk for themed widgets
        self.style = ttk.Style()
        self.style.theme_use('clam') # 'clam' is a good base for customization

        # Configure default font for ttk widgets
        self.style.configure('.', font=('Segoe UI', 10))
        self.style.configure('TButton', font=('Segoe UI', 10, 'bold'))
        self.style.configure('TLabel', font=('Segoe UI', 10))
        self.style.configure('TCheckbutton', font=('Segoe UI', 10))
        self.style.configure('TEntry', font=('Segoe UI', 10))
        self.style.configure('TFrame', background='#f0f0f0') # Default light mode background
        self.style.configure('TLabelframe', background='#f0f0f0')
        self.style.configure('TLabelframe.Label', background='#f0f0f0')

        self.log_clear_timer_id = None # For auto-clearing logs
        self.dark_mode = tk.BooleanVar(value=False)

        self.log_queue = queue.Queue()
        self.organizer = None
        self.thread = None

        # Main controls frame
        controls_frame = ttk.LabelFrame(root, text="Configuration")
        controls_frame.pack(fill=tk.X, padx=10, pady=8)

        # Path selection
        path_frame = ttk.Frame(controls_frame)
        path_frame.pack(fill=tk.X, padx=5, pady=5)
        ttk.Label(path_frame, text='Base Path:').pack(side=tk.LEFT, padx=(0, 5))
        self.path_var = tk.StringVar() 
        self.path_entry = ttk.Entry(path_frame, textvariable=self.path_var, width=60)
        self.path_entry.pack(side=tk.LEFT, expand=True, fill=tk.X, padx=(0, 5))
        ttk.Button(path_frame, text='Browse', command=self.browse).pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(path_frame, text='Auto-detect', command=self.autodetect_path).pack(side=tk.LEFT)

        # Options frame
        options_frame = ttk.Frame(controls_frame)
        options_frame.pack(fill=tk.X, padx=5, pady=5)

        # Interval
        ttk.Label(options_frame, text='Watch Interval (s):').grid(row=0, column=0, sticky='w', padx=(0, 5))
        self.interval_var = tk.IntVar(value=5)
        ttk.Entry(options_frame, textvariable=self.interval_var, width=10).grid(row=0, column=1, sticky='w', padx=(0, 15))

        # Single folder / Scan all months
        self.single_var = tk.BooleanVar(value=False)
        ttk.Checkbutton(options_frame, text='Organize a Single Folder (not YYYY-MM structure)', variable=self.single_var).grid(row=1, column=0, columnspan=2, sticky='w', pady=(5,0))

        self.scan_all_months_var = tk.BooleanVar(value=False) # New toggle
        ttk.Checkbutton(options_frame, text='Scan All Month Folders (default is latest month only)', variable=self.scan_all_months_var).grid(row=2, column=0, columnspan=2, sticky='w', pady=(0,5))

        # Manual Organization Structure (Template)
        self.template_var = tk.StringVar(value="{world}")
        ttk.Label(options_frame, text='Subfolder Template:').grid(row=3, column=0, sticky='w', padx=(0, 5))
        self.template_entry = ttk.Entry(options_frame, textvariable=self.template_var, width=30)
        self.template_entry.grid(row=3, column=1, sticky='w')

        # Theme toggle
        ttk.Checkbutton(options_frame, text='Dark Mode', variable=self.dark_mode, command=self.apply_theme).grid(row=0, column=2, sticky='e', padx=(20,0))

        # Buttons
        btn_frame = ttk.Frame(root)
        btn_frame.pack(fill=tk.X, padx=10, pady=5)

        self.start_btn = ttk.Button(btn_frame, text='▶ Start Watch', command=self.start_watch, style='Accent.TButton')
        self.start_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.stop_btn = ttk.Button(btn_frame, text='⏹ Stop', command=self.stop_watch, state=tk.DISABLED)
        self.stop_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.run_btn = ttk.Button(btn_frame, text='⟳ Run Once', command=self.run_once)
        self.run_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.preview_btn = ttk.Button(btn_frame, text='🔍 Preview (Dry-Run)', command=self.preview)
        self.preview_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.autostart_btn = ttk.Button(btn_frame, text='⚙️ Install Autostart', command=self.install_autostart)
        self.autostart_btn.pack(side=tk.LEFT)
        self.clear_log_btn = ttk.Button(btn_frame, text='🗑️ Clear Log', command=self.clear_log)
        self.clear_log_btn.pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(btn_frame, text='❓ Help', command=self.show_help).pack(side=tk.LEFT)

        # Stats
        stats_frame = ttk.Frame(root)
        stats_frame.pack(fill=tk.X, padx=10, pady=5)
        self.stats_label = ttk.Label(stats_frame, text='Processed: 0 | Organized: 0 | No metadata: 0 | Errors: 0')
        self.stats_label.pack(side=tk.LEFT, anchor='w')

        self.progress = ttk.Progressbar(stats_frame, mode='determinate', length=150)
        self.progress.pack(side=tk.RIGHT, padx=5)

        # Handle window closing to save settings
        root.protocol("WM_DELETE_WINDOW", self.on_closing)

        # Log area
        self.log = scrolledtext.ScrolledText(root, height=15, wrap=tk.WORD, state=tk.DISABLED) # Disable editing
        self.log.pack(fill=tk.BOTH, expand=True, padx=10, pady=8)

        # Apply initial theme
        self.apply_theme()

        # Configure root logger for GUI
        root_logger = logging.getLogger()
        root_logger.setLevel(logging.INFO)
        for h in root_logger.handlers[:]:
            root_logger.removeHandler(h)

        gui_handler = TextHandler(self.log_queue)
        gui_handler.setFormatter(logging.Formatter('%(asctime)s - %(levelname)s - %(message)s', datefmt='%H:%M:%S'))
        root_logger.addHandler(gui_handler)

        # Periodic UI update for stats
        self.update_stats()

        # Initial button states
        self._set_ui_state("idle")
        
        # Load saved settings
        self.load_settings()

        self._schedule_log_clear() # Start the initial log clear timer
        
        # Auto-detect path on startup silently if no valid path is loaded
        current_path = self.path_var.get()
        if not current_path or not Path(current_path).exists():
            self.autodetect_path(silent=True)
            
        # Start polling the log queue
        self.poll_log_queue()
        self.add_tooltips()

    def add_tooltips(self):
        """Add tooltips to UI elements."""
        ToolTip(self.path_entry, "The main folder where your VRChat screenshots are saved.")
        ToolTip(self.template_entry, "Subfolder structure. Variables: {world}, {year}, {month}, {day}, {width}, {height}")
        ToolTip(self.start_btn, "Begin monitoring your folder for new screenshots in real-time.")
        ToolTip(self.stop_btn, "Stop the active folder monitoring.")
        ToolTip(self.run_btn, "Scan and organize existing screenshots once without watching.")
        ToolTip(self.preview_btn, "See what changes would be made without moving any files.")
        ToolTip(self.autostart_btn, "Configure this app to start automatically when you log in.")
        ToolTip(self.clear_log_btn, "Clear the message history shown in the window below.")
        ToolTip(self.progress, "Indicates when the organizer is actively processing files.")

    def get_config_path(self):
        """Get path for the settings file."""
        if getattr(sys, 'frozen', False):
            # If running as a bundled executable (EXE or AppImage)
            base_dir = os.path.dirname(sys.executable)
        else:
            # If running as a normal python script
            base_dir = os.path.dirname(os.path.abspath(__file__))
        return Path(base_dir) / "gui_settings.json"

    def load_settings(self):
        """Load settings from JSON file."""
        config_path = self.get_config_path()
        if config_path.exists():
            try:
                with open(config_path, 'r') as f:
                    data = json.load(f)
                    self.path_var.set(data.get('path', ''))
                    self.interval_var.set(data.get('interval', 5))
                    self.single_var.set(data.get('single_folder', False))
                    self.scan_all_months_var.set(data.get('scan_all_months', False))
                    self.template_var.set(data.get('template', '{world}'))
                    self.dark_mode.set(data.get('dark_mode', False))
                    self.apply_theme() # Re-apply theme after loading dark_mode state
            except Exception as e:
                logger.error(f"Failed to load settings: {e}")

    def save_settings(self):
        """Save current settings to JSON file."""
        config_path = self.get_config_path()
        data = {
            'path': self.path_var.get(),
            'interval': self.interval_var.get(),
            'single_folder': self.single_var.get(),
            'scan_all_months': self.scan_all_months_var.get(),
            'template': self.template_var.get(),
            'dark_mode': self.dark_mode.get()
        }
        try:
            with open(config_path, 'w') as f:
                json.dump(data, f, indent=4)
        except Exception as e:
            logger.error(f"Failed to save settings: {e}")

    def poll_log_queue(self):
        """Check the log queue and update the text widget."""
        try:
            while True:
                msg = self.log_queue.get_nowait()
                self.log.config(state=tk.NORMAL)
                self.log.insert(tk.END, msg + '\n')
                self.log.config(state=tk.DISABLED)
                self.log.see(tk.END)
                self._schedule_log_clear() # Reset timer on new log entry
        except queue.Empty:
            pass
        finally:
            self.root.after(100, self.poll_log_queue)
            
    def _schedule_log_clear(self):
        """Schedules the log to be cleared after inactivity."""
        if self.log_clear_timer_id:
            self.root.after_cancel(self.log_clear_timer_id)
        # Schedule to clear after 30 seconds (30000 ms)
        self.log_clear_timer_id = self.root.after(30000, self._clear_log_if_inactive)

    def _clear_log_if_inactive(self):
        """Clears the log if no new activity has occurred."""
        # Only clear if the log is not empty
        if self.root.winfo_exists() and self.log.compare("end-1c", "!=", "1.0"): # Check if there's any text
            self.clear_log()
            logger.info("Log cleared due to inactivity.")
        self.log_clear_timer_id = None # Reset timer ID after clearing

    def clear_log(self):
        """Clears the content of the log area."""
        self.log.config(state=tk.NORMAL)
        self.log.delete(1.0, tk.END)
        self.log.config(state=tk.DISABLED)

    def _set_ui_state(self, state: str):
        """Update UI elements based on current application state: idle, busy, watching."""
        if state == "watching":
            self.start_btn.config(state=tk.DISABLED)
            self.stop_btn.config(state=tk.NORMAL)
            self.run_btn.config(state=tk.DISABLED)
            self.preview_btn.config(state=tk.DISABLED)
        elif state == "busy":
            self.start_btn.config(state=tk.DISABLED)
            self.stop_btn.config(state=tk.DISABLED)
            self.run_btn.config(state=tk.DISABLED)
            self.preview_btn.config(state=tk.DISABLED)
        else: # idle
            self.start_btn.config(state=tk.NORMAL)
            self.stop_btn.config(state=tk.DISABLED)
            self.run_btn.config(state=tk.NORMAL)
            self.preview_btn.config(state=tk.NORMAL)
            self.progress.stop()
            self.progress['value'] = 0

    def browse(self):
        # start directory in sensible default (existing entry or Pictures/VRChat)
        current = self.path_var.get() or str(Path.home() / 'Pictures' / 'VRChat' / 'VRChat')
        p = filedialog.askdirectory(initialdir=current)
        if p:
            self.path_var.set(p)

    def autodetect_path(self, silent=False):
        """Try to find the VRChat pictures folder automatically."""
        # Common default
        candidates = []
        home = Path.home()
        candidates.append(home / 'Pictures' / 'VRChat' / 'VRChat')
        candidates.append(home / 'Pictures' / 'VRChat')
        candidates.append(home / 'Pictures')

        # Search for directories named 'VRChat' under Pictures (one level deep)
        pictures = home / 'Pictures'
        try:
            if pictures.exists():
                for p in pictures.iterdir():
                    if p.is_dir() and 'vrchat' in p.name.lower():
                        candidates.append(p)
                        for sub in p.iterdir():
                            if sub.is_dir() and 'vrchat' in sub.name.lower():
                                candidates.append(sub)
        except Exception:
            pass

        # Also look for YYYY-MM folders which indicate VRChat structure
        def looks_like_vrchat_folder(p: Path):
            if not p.exists() or not p.is_dir():
                return False
            for sub in p.iterdir():
                if sub.is_dir() and len(sub.name) == 7 and sub.name[4] == '-':
                    return True
            return False

        chosen = None
        for cand in candidates:
            try:
                if looks_like_vrchat_folder(cand):
                    chosen = cand
                    break
            except Exception:
                continue

        if not chosen:
            # fallback: first existing candidate
            for cand in candidates:
                if cand.exists():
                    chosen = cand
                    break

        if chosen:
            self.path_var.set(str(chosen))
            if not silent:
                messagebox.showinfo('Auto-detect', f'Auto-detected path: {chosen}')
            logger.info('Auto-detected VRChat path: %s', chosen)
        else:
            if not silent:
                messagebox.showwarning('Auto-detect', 'Could not auto-detect VRChat pictures folder')

    def ensure_organizer(self):
        base = self.path_var.get() or str(Path.home() / 'Pictures' / 'VRChat' / 'VRChat')
        if not self.organizer or str(self.organizer.base_path) != str(base):
            self.organizer = VRChatOrganizer(base)
        return self.organizer

    def start_watch(self):
        if self.thread and self.thread.is_alive():
            logger.info('Watch already running')
            return

        # Added confirmation warning
        confirm = messagebox.askyesno(
            "Confirm Watch Mode",
            "Are you sure you want to start Watch Mode?\n\n"
            "It is highly recommended to run a 'Preview (Dry-Run)' first if you haven't already to ensure the organization logic matches your expectations."
        )
        if not confirm:
            return

        org = self.ensure_organizer()
        interval = max(1, int(self.interval_var.get()))
        single = self.single_var.get()
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(), # Pass new toggle
            'dry_run': False,
            'watch': True,
            'interval': interval,
            'template': self.template_var.get()
        }
        def target():
            try:
                org.run(**args)
            except Exception as e:
                logger.exception('Organizer thread error: %s', e)
        logger.info('Starting background watch process...')
        self._set_ui_state("watching")
        self.thread = threading.Thread(target=target, daemon=True)
        self.thread.start()

    def stop_watch(self):
        if not self.organizer:
            return
        self.organizer.stop()
        self._set_ui_state("idle")

    def run_once(self):
        org = self.ensure_organizer()
        single = self.single_var.get()
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(),
            'dry_run': False,
            'watch': False,
            'template': self.template_var.get()
        }
        
        def task():
            try:
                org.run(**args)
            finally:
                self.root.after(0, lambda: self._set_ui_state("idle"))

        self._set_ui_state("busy")
        threading.Thread(target=task, daemon=True).start()
        logger.info('Running one-time organization...')

    def preview(self):
        org = self.ensure_organizer()
        single = self.single_var.get()
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(),
            'dry_run': True,
            'watch': False,
            'template': self.template_var.get()
        }

        def task():
            try:
                org.run(**args)
            finally:
                self.root.after(0, lambda: self._set_ui_state("idle"))

        self._set_ui_state("busy")
        threading.Thread(target=task, daemon=True).start()
        logger.info('Generating dry-run preview...')

    def install_autostart(self):
        initial = os.path.join(os.getcwd(), 'VRChatOrganizer.AppImage')
        exe_path = filedialog.askopenfilename(
            title='Select executable to run on login (AppImage or Python script)',
            initialdir=os.getcwd(),
            initialfile=os.path.basename(initial),
            filetypes=[('All files', '*')]
        )
        if not exe_path:
            return

        # Windows: create a startup batch in the user's Startup folder
        if sys.platform.startswith('win') or os.name == 'nt':
            try:
                startup_dir = os.path.join(os.getenv('APPDATA'), 'Microsoft', 'Windows', 'Start Menu', 'Programs', 'Startup')
                os.makedirs(startup_dir, exist_ok=True)
                bat_name = 'VRChatOrganizer-startup.bat'
                bat_path = os.path.join(startup_dir, bat_name)

                # Choose invocation depending on selected file type
                if exe_path.lower().endswith('.py'):
                    cmd = f'"{sys.executable}" "{exe_path}" --watch'
                else:
                    cmd = f'"{exe_path}" --watch'

                with open(bat_path, 'w') as f:
                    f.write('@echo off\n')
                    f.write(cmd + '\n')

                messagebox.showinfo('Autostart Installed', f'Created startup entry: {bat_path}')
                logger.info('Created Windows startup batch: %s', bat_path)
                return
            except Exception as e:
                messagebox.showerror('Error', f'Failed to create Windows startup entry: {e}')
                logger.exception('Error creating Windows startup entry: %s', e)

        # Default: assume systemd user services (Linux)
        service_name = 'vrchat-organizer.service'
        unit = f'''[Unit]
Description=VRChat Organizer (user)

[Service]
Type=simple
ExecStart={exe_path} --watch
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
'''

        user_systemd_dir = Path.home() / '.config' / 'systemd' / 'user'
        try:
            user_systemd_dir.mkdir(parents=True, exist_ok=True)
            unit_path = user_systemd_dir / service_name
            with open(unit_path, 'w') as f:
                f.write(unit)

            # Reload user systemd, enable and start
            subprocess.run(['systemctl', '--user', 'daemon-reload'], check=True)
            subprocess.run(['systemctl', '--user', 'enable', '--now', service_name], check=True)

            messagebox.showinfo('Autostart Installed', f'Enabled {service_name} for your user.')
            logger.info('Installed systemd user service: %s', unit_path)
        except subprocess.CalledProcessError as e:
            messagebox.showerror('Failed to enable service', f'systemctl error: {e}')
            logger.exception('systemctl error while installing autostart: %s', e)
        except Exception as e:
            messagebox.showerror('Error', f'Failed to write service file: {e}')
            logger.exception('Failed to write systemd user service: %s', e)

    def show_help(self):
        """Show help information."""
        help_text = (
            "VRChat Organizer Help\n\n"
            "• Base Path: Where your VRChat screenshots live (e.g. Pictures/VRChat/VRChat).\n"
            "• Watch Interval: How often (in seconds) the app checks for new images.\n"
            "• Single Folder: If checked, the app organizes the target folder directly instead of looking for YYYY-MM subfolders.\n"
            "• Scan All Months: Organizes your entire screenshot history instead of just the latest month.\n"
            "• Subfolder Template: How world folders are named. Use {world} as a placeholder for the VRChat world name. You can also use {year}, {month}, {day} for the screenshot's date, and {width}, {height} for its dimensions.\n\n"
            "💡 Pro Tip: Hover over any button for a quick hint!"
        )
        messagebox.showinfo("About / Help", help_text)

    def update_stats(self):
        stats = {
            'processed': 0,
            'organized': 0,
            'no_metadata': 0,
            'errors': 0,
            'total': 0
        }
        if self.organizer and hasattr(self.organizer, 'stats'):
            try:
                stats.update(self.organizer.stats.copy())
            except Exception:
                pass

        self.stats_label.config(
            text=f"Processed: {stats['processed']} | Organized: {stats['organized']} | No metadata: {stats['no_metadata']} | Errors: {stats['errors']}"
        )

        # Update progress bar
        if stats['total'] > 0:
            self.progress['maximum'] = stats['total']
            self.progress['value'] = stats['processed']
        else:
            self.progress['value'] = 0

        # Schedule next update
        try:
            self.root.after(1000, self.update_stats)
        except Exception:
            pass # GUI might be closing

    def apply_theme(self):
        dark = bool(self.dark_mode.get())
        if dark:
            # Dark theme colors
            bg_color = '#2e2e2e'
            fg_color = '#eaeaea'
            entry_bg_color = '#3a3a3a'
            entry_fg_color = '#eaeaea'
            text_bg_color = '#202020'
            text_fg_color = '#eaeaea'
            button_bg_color = '#4a4a4a'
            button_fg_color = '#ffffff'
            accent_button_bg_color = '#007bff' # A blue accent
        else:
            # Light theme colors
            bg_color = '#f0f0f0'
            fg_color = '#000000'
            entry_bg_color = '#ffffff'
            entry_fg_color = '#000000'
            text_bg_color = '#ffffff'
            text_fg_color = '#000000'
            button_bg_color = '#e0e0e0'
            button_fg_color = '#000000'
            accent_button_bg_color = '#007bff' # Still blue accent

        try:
            # Configure ttk styles
            self.style.configure('TFrame', background=bg_color)
            self.style.configure('TLabelframe', background=bg_color, foreground=fg_color)
            self.style.configure('TLabelframe.Label', background=bg_color, foreground=fg_color)
            self.style.configure('TLabel', background=bg_color, foreground=fg_color)
            self.style.configure('TCheckbutton', background=bg_color, foreground=fg_color)
            self.style.map('TCheckbutton',
                           background=[('active', bg_color)],
                           foreground=[('active', fg_color)])

            self.style.configure('TEntry', fieldbackground=entry_bg_color, foreground=entry_fg_color)

            # General button style
            self.style.configure('TButton',
                                 background=button_bg_color,
                                 foreground=button_fg_color,
                                 bordercolor=button_bg_color,
                                 lightcolor=button_bg_color,
                                 darkcolor=button_bg_color,
                                 relief='flat',
                                 padding=(10, 5))
            self.style.map('TButton',
                           background=[('active', '#6a6a6a' if dark else '#c0c0c0')],
                           foreground=[('active', button_fg_color)])

            # Accent button style (for Start Watch)
            self.style.configure('Accent.TButton',
                                 background=accent_button_bg_color,
                                 foreground='#ffffff',
                                 bordercolor=accent_button_bg_color)
            self.style.map('Accent.TButton',
                           background=[('active', '#0056b3')]) # Darker blue on hover

            # ScrolledText (not a ttk widget)
            self.log.config(bg=text_bg_color, fg=text_fg_color, insertbackground=text_fg_color)
            self.root.config(bg=bg_color) # Root window background
        except Exception:
            pass

    def on_closing(self):
        """Action to perform when the window is closed."""
        self.save_settings()
        self.root.destroy()

if __name__ == '__main__':
    root = tk.Tk()
    app = App(root)
    root.mainloop()
