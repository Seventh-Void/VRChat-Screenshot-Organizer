#!/usr/bin/env python3
"""Simple Tkinter GUI for VRChat Organizer"""
import threading
import queue
import logging
import sys
import os
import subprocess
from pathlib import Path # Keep Path for file operations
import tkinter as tk # Keep tk for root, BooleanVar, StringVar, IntVar, messagebox, scrolledtext
from tkinter import filedialog, scrolledtext, messagebox, font, ttk # Add ttk

from organize_vrchat import VRChatOrganizer

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
        self.path_var = tk.StringVar(value=str(Path.home() / 'Pictures' / 'VRChat' / 'VRChat')) # Default path
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

        # Software filter
        self.software_var = tk.StringVar()
        ttk.Label(options_frame, text='Software Filter (optional):').grid(row=3, column=0, sticky='w', padx=(0, 5))
        ttk.Entry(options_frame, textvariable=self.software_var, width=30).grid(row=3, column=1, sticky='w')

        # Theme toggle
        ttk.Checkbutton(options_frame, text='Dark Mode', variable=self.dark_mode, command=self.apply_theme).grid(row=0, column=2, sticky='e', padx=(20,0))

        # Buttons
        btn_frame = ttk.Frame(root)
        btn_frame.pack(fill=tk.X, padx=10, pady=5)

        self.start_btn = ttk.Button(btn_frame, text='▶ Start Watch', command=self.start_watch, style='Accent.TButton')
        self.start_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.stop_btn = ttk.Button(btn_frame, text='■ Stop', command=self.stop_watch, state=tk.DISABLED)
        self.stop_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.run_btn = ttk.Button(btn_frame, text='⟳ Run Once', command=self.run_once)
        self.run_btn.pack(side=tk.LEFT, padx=(0, 5))
        self.preview_btn = ttk.Button(btn_frame, text='🔍 Preview (Dry-Run)', command=self.preview)
        self.preview_btn.pack(side=tk.LEFT, padx=(0, 5))
        ttk.Button(btn_frame, text='⚙️ Install Autostart', command=self.install_autostart).pack(side=tk.LEFT)

        # Stats
        stats_frame = ttk.Frame(root)
        stats_frame.pack(fill=tk.X, padx=10, pady=5)
        self.stats_label = ttk.Label(stats_frame, text='Processed: 0 | Organized: 0 | No metadata: 0 | Errors: 0')
        self.stats_label.pack(side=tk.LEFT, anchor='w')

        self.progress = ttk.Progressbar(stats_frame, mode='indeterminate', length=150)
        self.progress.pack(side=tk.RIGHT, padx=5)

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
        
        # Start polling the log queue
        self.poll_log_queue()

    def poll_log_queue(self):
        """Check the log queue and update the text widget."""
        try:
            while True:
                msg = self.log_queue.get_nowait()
                self.log.config(state=tk.NORMAL)
                self.log.insert(tk.END, msg + '\n')
                self.log.config(state=tk.DISABLED)
                self.log.see(tk.END)
        except queue.Empty:
            pass
        finally:
            self.root.after(100, self.poll_log_queue)

    def _set_ui_state(self, state: str):
        """Update UI elements based on current application state: idle, busy, watching."""
        if state == "watching":
            self.start_btn.config(state=tk.DISABLED)
            self.stop_btn.config(state=tk.NORMAL)
            self.run_btn.config(state=tk.DISABLED)
            self.preview_btn.config(state=tk.DISABLED)
            self.progress.start(10)
        elif state == "busy":
            self.start_btn.config(state=tk.DISABLED)
            self.stop_btn.config(state=tk.DISABLED)
            self.run_btn.config(state=tk.DISABLED)
            self.preview_btn.config(state=tk.DISABLED)
            self.progress.start(10)
        else: # idle
            self.start_btn.config(state=tk.NORMAL)
            self.stop_btn.config(state=tk.DISABLED)
            self.run_btn.config(state=tk.NORMAL)
            self.preview_btn.config(state=tk.NORMAL)
            self.progress.stop()

    def browse(self):
        # start directory in sensible default (existing entry or Pictures/VRChat)
        current = self.path_var.get() or str(Path.home() / 'Pictures' / 'VRChat' / 'VRChat')
        p = filedialog.askdirectory(initialdir=current)
        if p:
            self.path_var.set(p)

    def autodetect_path(self):
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
            messagebox.showinfo('Auto-detect', f'Auto-detected path: {chosen}')
            logger.info('Auto-detected VRChat path: %s', chosen)
        else:
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
        org = self.ensure_organizer()
        interval = max(1, int(self.interval_var.get()))
        single = self.single_var.get()
        software = self.software_var.get() or None
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(), # Pass new toggle
            'software_filter': software,
            'dry_run': False,
            'watch': True,
            'interval': interval,
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
        software = self.software_var.get() or None
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(),
            'software_filter': software,
            'dry_run': False,
            'watch': False,
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
        software = self.software_var.get() or None
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'scan_all_months': self.scan_all_months_var.get(),
            'software_filter': software,
            'dry_run': True,
            'watch': False,
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

    def update_stats(self):
        stats = {
            'processed': 0,
            'organized': 0,
            'no_metadata': 0,
            'errors': 0,
        }
        if self.organizer and hasattr(self.organizer, 'stats'):
            try:
                stats.update(self.organizer.stats.copy())
            except Exception:
                pass

        self.stats_label.config(
            text=f"Processed: {stats['processed']} | Organized: {stats['organized']} | No metadata: {stats['no_metadata']} | Errors: {stats['errors']}"
        )
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

if __name__ == '__main__':
    root = tk.Tk()
    app = App(root)
    root.mainloop()
