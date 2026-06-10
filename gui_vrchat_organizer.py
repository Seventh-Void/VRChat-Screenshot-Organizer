#!/usr/bin/env python3
"""Simple Tkinter GUI for VRChat Organizer"""
import threading
import logging
import sys
import os
import subprocess
from pathlib import Path
import tkinter as tk
from tkinter import filedialog, scrolledtext, messagebox, font

from organize_vrchat import VRChatOrganizer

# Configure logging for GUI
logger = logging.getLogger('vrchat_gui')
logger.setLevel(logging.INFO)

class TextHandler(logging.Handler):
    def __init__(self, text_widget):
        super().__init__()
        self.text_widget = text_widget

    def emit(self, record):
        msg = self.format(record)
        def append():
            self.text_widget.insert(tk.END, msg + '\n')
            self.text_widget.yview(tk.END)
        try:
            self.text_widget.after(0, append)
        except Exception:
            pass

class App:
    def __init__(self, root):
        self.root = root
        root.title('VRChat Organizer')

        # visual tweaks
        default_font = font.nametofont('TkDefaultFont')
        default_font.configure(size=10)
        root.option_add('*Font', default_font)

        self.dark_mode = tk.BooleanVar(value=False)

        self.organizer = None
        self.thread = None

        # Controls frame
        frame = tk.Frame(root)
        frame.pack(fill=tk.X, padx=8, pady=6)

        tk.Label(frame, text='Path:').grid(row=0, column=0, sticky='w')
        self.path_var = tk.StringVar()
        self.path_entry = tk.Entry(frame, textvariable=self.path_var, width=50)
        self.path_entry.grid(row=0, column=1, padx=4)
        tk.Button(frame, text='Browse', command=self.browse).grid(row=0, column=2)
        tk.Button(frame, text='Auto-detect', command=self.autodetect_path).grid(row=0, column=3, padx=6)

        tk.Label(frame, text='Interval (s):').grid(row=1, column=0, sticky='w')
        self.interval_var = tk.IntVar(value=30)
        tk.Entry(frame, textvariable=self.interval_var, width=8).grid(row=1, column=1, sticky='w')

        self.single_var = tk.BooleanVar()
        tk.Checkbutton(frame, text='Single folder', variable=self.single_var).grid(row=2, column=1, sticky='w')

        self.software_var = tk.StringVar()
        tk.Label(frame, text='Software filter:').grid(row=3, column=0, sticky='w')
        tk.Entry(frame, textvariable=self.software_var, width=30).grid(row=3, column=1, sticky='w')
        tk.Checkbutton(frame, text='Dark mode', variable=self.dark_mode, command=self.apply_theme).grid(row=3, column=3, sticky='w')

        # Buttons
        btn_frame = tk.Frame(root)
        btn_frame.pack(fill=tk.X, padx=8, pady=6)
        self.start_btn = tk.Button(btn_frame, text='Start Watch', command=self.start_watch)
        self.start_btn.pack(side=tk.LEFT)
        tk.Button(btn_frame, text='Stop', command=self.stop_watch).pack(side=tk.LEFT, padx=6)
        tk.Button(btn_frame, text='Run Once', command=self.run_once).pack(side=tk.LEFT, padx=6)
        tk.Button(btn_frame, text='Preview (dry-run)', command=self.preview).pack(side=tk.LEFT, padx=6)
        tk.Button(btn_frame, text='Install Autostart', command=self.install_autostart).pack(side=tk.LEFT, padx=6)

        # Stats
        stats_frame = tk.Frame(root)
        stats_frame.pack(fill=tk.X, padx=8, pady=6)
        self.stats_label = tk.Label(stats_frame, text='Processed: 0 | Organized: 0 | No metadata: 0 | Errors: 0')
        self.stats_label.pack(anchor='w')

        # Log area
        self.log = scrolledtext.ScrolledText(root, height=16)
        self.log.pack(fill=tk.BOTH, expand=True, padx=8, pady=6)

        # Apply initial theme
        self.apply_theme()

        # attach logging handler
        handler = TextHandler(self.log)
        handler.setFormatter(logging.Formatter('%(asctime)s - %(levelname)s - %(message)s'))
        logger.addHandler(handler)

        # Periodic UI update for stats
        self.update_stats()

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
            # attach organizer logger to GUI
            org_logger = logging.getLogger()
            org_logger.setLevel(logging.INFO)
            # prevent adding multiple handlers
            if not any(isinstance(h, TextHandler) for h in org_logger.handlers):
                org_logger.addHandler(TextHandler(self.log))
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
        self.thread = threading.Thread(target=target, daemon=True)
        self.thread.start()
        logger.info('Started watch thread')

    def stop_watch(self):
        if not self.organizer:
            logger.info('No organizer running')
            return
        self.organizer.stop()
        logger.info('Stop requested')

    def run_once(self):
        org = self.ensure_organizer()
        single = self.single_var.get()
        software = self.software_var.get() or None
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'software_filter': software,
            'dry_run': False,
            'watch': False,
        }
        threading.Thread(target=lambda: org.run(**args), daemon=True).start()
        logger.info('Started single-run thread')

    def preview(self):
        org = self.ensure_organizer()
        single = self.single_var.get()
        software = self.software_var.get() or None
        args = {
            'single_folder': Path(self.path_var.get()) if single else None,
            'software_filter': software,
            'dry_run': True,
            'watch': False,
        }
        threading.Thread(target=lambda: org.run(**args), daemon=True).start()
        logger.info('Started preview (dry-run) thread')

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
                stats.update(self.organizer.stats)
            except Exception:
                pass

        self.stats_label.config(
            text=f"Processed: {stats['processed']} | Organized: {stats['organized']} | No metadata: {stats['no_metadata']} | Errors: {stats['errors']}"
        )
        # Schedule next update
        try:
            self.root.after(1000, self.update_stats)
        except Exception:
            pass

    def apply_theme(self):
        dark = bool(self.dark_mode.get())
        if dark:
            bg = '#2e2e2e'
            fg = '#eaeaea'
            entry_bg = '#3a3a3a'
            txt_bg = '#202020'
        else:
            bg = '#f0f0f0'
            fg = '#000000'
            entry_bg = '#ffffff'
            txt_bg = '#ffffff'

        try:
            self.root.configure(bg=bg)
            for widget in self.root.winfo_children():
                try:
                    widget.configure(bg=bg)
                except Exception:
                    pass

            # Specific widgets
            self.log.configure(bg=txt_bg, fg=fg, insertbackground=fg)
            self.path_entry.configure(bg=entry_bg, fg=fg)
            self.stats_label.configure(bg=bg, fg=fg)
        except Exception:
            pass

if __name__ == '__main__':
    root = tk.Tk()
    app = App(root)
    root.mainloop()
