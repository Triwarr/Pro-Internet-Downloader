// Pro Internet Manager Setup - portable single-exe installer / uninstaller (by Triwar Design)
// C# 5 / .NET Framework 4.8 WinForms, built with csc. Style based on InstallerRestyle.cs (Universal Upscaler look).
// Setup build: payload.zip + Uninstall.exe are embedded resources. Uninstall build: same source without payload.
//
// Command line:
//   Setup:     [/silent] [/dir=<folder>] [/test] [/noshortcuts] [/nodesktop] [/nostartmenu] [/noregistry] [/autostart] [/nochromium] [/run] [/log=<file>]
//              /test = no shortcuts, no registry, no launch (for test installs)
//   Internal:  /elevated (restarted via UAC), /autoinstall (GUI starts installing right away with the given options)
//   Every run is also logged to %TEMP%\PIM-Setup.log (no secrets: paths, steps and errors only).
//   A folder that is not writable is detected up front; under Program Files Setup restarts itself elevated (UAC).
//   Uninstall: Uninstall.exe [/silent] [/dir=<folder>] [/purge] [/log=<file>]   (or Setup.exe /uninstall /dir=...)
using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.Drawing;
using System.Drawing.Drawing2D;
using System.IO;
using System.IO.Compression;
using System.Net.NetworkInformation;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Threading;
using System.Windows.Forms;
using Microsoft.Win32;

internal static class Program
{
    [DllImport("user32.dll")]
    private static extern bool SetProcessDPIAware();

    [STAThread]
    private static int Main(string[] args)
    {
        Args a = Args.Parse(args);
#if PREVIEW
        if (a.Preview != null) { SetProcessDPIAware(); Application.EnableVisualStyles(); Application.SetCompatibleTextRenderingDefault(false); Preview.Run(a.Preview); return 0; }
#endif
        bool uninstall = a.Uninstall || !Engine.HasPayload();
        SetupLog.Start(args, uninstall, a.Silent);
        AppDomain.CurrentDomain.UnhandledException += delegate(object sender, UnhandledExceptionEventArgs ev) { SetupLog.Write("FATAL: " + ev.ExceptionObject); };
        if (a.Silent)
        {
            StreamWriter logFile = null;
            try
            {
                if (a.Log != null) { logFile = new StreamWriter(a.Log, false, new UTF8Encoding(false)); logFile.AutoFlush = true; }
                Action<string> log = delegate(string s) { SetupLog.Write(s); if (logFile != null) logFile.WriteLine(DateTime.Now.ToString("HH:mm:ss") + "  " + s); };
                Action<int, string> prog = delegate(int p, string s) { };
                string target = uninstall ? (a.Dir ?? Engine.SelfDir()) : (a.Dir ?? Engine.DefaultDir());
                List<string> busyApps = Engine.RunningIn(target);
                if (busyApps.Count > 0) { log("ERROR: Pro Internet Manager is running from " + target + " - close it first"); return 3; }
                Exception werr;
                if (!Engine.CanWrite(target, out werr))
                {
                    log("Folder is not writable: " + target + " (" + werr.Message + ")");
                    if (Engine.NeedsAdmin(target) && !Engine.IsAdmin() && !a.Elevated)
                    {
                        log("Folder is under Program Files: restarting Setup with administrator rights (UAC)");
                        if (logFile != null) { logFile.Dispose(); logFile = null; }   // the elevated copy writes the same /log= file
                        int code = Engine.RelaunchElevated(Engine.JoinArgs(args) + " /elevated", true, log);
                        log("Elevated Setup finished with exit code " + code);
                        return code;
                    }
                    log("ERROR: Setup cannot write to " + target + ". Use /dir= with a writable folder (for example " + Engine.UserDir() + ") or run Setup as administrator.");
                    return 5;
                }
                if (uninstall)
                {
                    Engine.Uninstall(target, a.Purge, log, prog);
                }
                else
                {
                    InstallOptions o = new InstallOptions();
                    o.Dir = target;
                    o.Desktop = !a.Test && !a.NoShortcuts && !a.NoDesktop;
                    o.StartMenu = !a.Test && !a.NoShortcuts && !a.NoStartMenu;
                    o.Registry = !a.Test && !a.NoRegistry;
                    o.Autostart = o.Registry && a.Autostart;
                    o.Chromium = !a.NoChromium;
                    Engine.Install(o, log, prog);
                    if (a.Run && !a.Test) Engine.Launch(o.Dir);
                }
                log("Done");
                return 0;
            }
            catch (Exception e)
            {
                SetupLog.Write("ERROR: " + e);
                if (logFile != null) logFile.WriteLine("ERROR: " + e);
                return 1;
            }
            finally { if (logFile != null) logFile.Dispose(); SetupLog.Write("Exit (silent)"); }
        }
        SetProcessDPIAware();
        Application.EnableVisualStyles();
        Application.SetCompatibleTextRenderingDefault(false);
        Application.SetUnhandledExceptionMode(UnhandledExceptionMode.CatchException);
        Application.ThreadException += delegate(object sender, ThreadExceptionEventArgs ev)
        {
            SetupLog.Write("ERROR (window): " + ev.Exception);
            MessageBox.Show(ev.Exception.Message + "\n\nDetails are in the log:\n" + SetupLog.FilePath, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Error);
        };
        Application.Run(new SetupForm(uninstall, a));
        SetupLog.Write("Exit (window)");
        return 0;
    }
}

internal sealed class Args
{
    public bool Silent, Uninstall, Test, NoShortcuts, NoDesktop, NoStartMenu, NoRegistry, Autostart, NoChromium, Run, Purge, Elevated, AutoInstall;
    public string Dir, Log, Preview;

    public static Args Parse(string[] args)
    {
        Args a = new Args();
        for (int i = 0; i < args.Length; i++)
        {
            string s = args[i];
            string l = s.ToLowerInvariant();
            if (l == "/silent" || l == "/s" || l == "/quiet") a.Silent = true;
            else if (l == "/uninstall") a.Uninstall = true;
            else if (l == "/test") a.Test = true;
            else if (l == "/noshortcuts") a.NoShortcuts = true;
            else if (l == "/noregistry") a.NoRegistry = true;
            else if (l == "/nodesktop") a.NoDesktop = true;
            else if (l == "/nostartmenu") a.NoStartMenu = true;
            else if (l == "/elevated") a.Elevated = true;
            else if (l == "/autoinstall") a.AutoInstall = true;
            else if (l == "/autostart") a.Autostart = true;
            else if (l == "/nochromium") a.NoChromium = true;
            else if (l == "/run") a.Run = true;
            else if (l == "/purge") a.Purge = true;
            else if (l.StartsWith("/dir=")) a.Dir = Path.GetFullPath(s.Substring(5).Trim('"').TrimEnd('\\'));
            else if (l.StartsWith("/log=")) a.Log = s.Substring(5).Trim('"');
            else if (l == "/preview" && i + 1 < args.Length) a.Preview = args[++i];
        }
        return a;
    }
}

// Plain-text log of every Setup run: %TEMP%\PIM-Setup.log (paths, steps and errors only, no secrets)
internal static class SetupLog
{
    private static readonly object gate = new object();
    private static readonly int pid = Process.GetCurrentProcess().Id;
    public static string FilePath { get { return Path.Combine(Path.GetTempPath(), "PIM-Setup.log"); } }

    public static void Start(string[] args, bool uninstall, bool silent)
    {
        try
        {
            FileInfo fi = new FileInfo(FilePath);
            if (fi.Exists && fi.Length > 2 * 1048576)
            {
                string old = Path.Combine(Path.GetTempPath(), "PIM-Setup.old.log");
                if (File.Exists(old)) File.Delete(old);
                File.Move(FilePath, old);
            }
        }
        catch { }
        Write("===== " + Engine.AppName + " Setup " + Engine.Version + " | " + (uninstall ? "uninstall" : "install") + (silent ? " (silent)" : " (window)")
            + " | admin: " + (Engine.IsAdmin() ? "yes" : "no") + " | exe: " + Assembly.GetExecutingAssembly().Location);
        Write("Arguments: " + (args.Length == 0 ? "(none)" : Engine.JoinArgs(args)));
    }

    public static void Write(string line)
    {
        try
        {
            lock (gate)
                File.AppendAllText(FilePath, DateTime.Now.ToString("yyyy-MM-dd HH:mm:ss.fff") + "  [" + pid + "]  " + line + "\r\n", new UTF8Encoding(false));
        }
        catch { }
    }
}

// A file or folder Setup could not write, with the path and the reason, shown to the user as is
internal sealed class SetupWriteException : Exception
{
    public readonly string TargetPath;
    public SetupWriteException(string what, string path, Exception inner)
        : base(Build(what, path, inner), inner) { TargetPath = path; }

    static string Build(string what, string path, Exception inner)
    {
        string msg = "Setup could not " + what + ":\n" + path + "\n\nReason: " + inner.Message;
        if (inner is UnauthorizedAccessException)
            msg += "\n\nWindows denied access to this folder. Choose a folder you can write to (for example " + Engine.UserDir() + ") or run Setup as administrator.";
        else
            msg += "\n\nThe file may be locked by another program (for example antivirus software). Try again in a moment.";
        return msg + "\n\nLog: " + SetupLog.FilePath;
    }
}

internal sealed class InstallOptions
{
    public string Dir;
    public bool Desktop = true, StartMenu = true, Autostart, Registry = true, Chromium = true;
}

// ---------------------------------------------------------------- install / uninstall logic (UI independent)
internal static class Engine
{
    public const string AppName = "Pro Internet Manager";
    public const string ExeName = "Pro Internet Manager.exe";
    public const string IconName = "ProInternetManager.ico";
    public const string UninstallExe = "Uninstall.exe";
    public const string ListFile = "uninstall.dat";
    public const string Version = "0.2.5";
    public const string Publisher = "Triwar Design";
    public const string RegKey = @"Software\Microsoft\Windows\CurrentVersion\Uninstall\ProInternetManager";
    public const string RunKey = @"Software\Microsoft\Windows\CurrentVersion\Run";
    public const string RunValue = "ProInternetManager";
    public const string XpiUrl = "https://addons.mozilla.org/firefox/downloads/file/5184228/87f89caacae34bb9b236-0.3.2.xpi";

    public static bool HasPayload()
    {
        using (Stream s = Assembly.GetExecutingAssembly().GetManifestResourceStream("payload.zip")) return s != null;
    }

    public static string SelfDir() { return Path.GetDirectoryName(Assembly.GetExecutingAssembly().Location); }

    public static string DefaultDir()
    {
        try
        {
            using (RegistryKey k = Registry.CurrentUser.OpenSubKey(RegKey))
            {
                string loc = k == null ? null : k.GetValue("InstallLocation") as string;
                if (!string.IsNullOrEmpty(loc)) return loc;
            }
        }
        catch { }
        return StdDir();
    }

    // 0.2.3: own data folder of Pro Internet Manager (logs, config, theme, update state). The folder
    // %APPDATA%\RustDownloadManager belongs to the original Rust Download Manager and is never touched.
    public static string SettingsDir() { return Path.Combine(DataRoot(Environment.SpecialFolder.ApplicationData, "Roaming"), "Pro Internet Downloader"); }
    // Downloaded updates and the updated yt-dlp copy.
    public static string LocalDataDir() { return Path.Combine(DataRoot(Environment.SpecialFolder.LocalApplicationData, "Local"), "ProInternetManager"); }
    // PIM_SETUP_TESTROOT=<folder> points both data folders into <folder>\Roaming and <folder>\Local (tests only).
    static string DataRoot(Environment.SpecialFolder folder, string testSub)
    {
        string test = Environment.GetEnvironmentVariable("PIM_SETUP_TESTROOT");
        return string.IsNullOrEmpty(test) ? Environment.GetFolderPath(folder) : Path.Combine(test, testSub);
    }

    static string Norm(string p) { return Path.GetFullPath(p).TrimEnd('\\') + "\\"; }

    // 0.2.5: default install folder is Program Files (Setup asks for UAC once).
    public static string StdDir()
    {
        string pf = Environment.GetEnvironmentVariable("ProgramW6432");
        if (string.IsNullOrEmpty(pf)) pf = Environment.GetFolderPath(Environment.SpecialFolder.ProgramFiles);
        return Path.Combine(pf, AppName);
    }
    // Per-user fallback folder that needs no administrator rights.
    public static string UserDir() { return Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.LocalApplicationData), "Programs", AppName); }

    public static bool IsAdmin()
    {
        try { using (WindowsIdentity id = WindowsIdentity.GetCurrent()) return new WindowsPrincipal(id).IsInRole(WindowsBuiltInRole.Administrator); }
        catch { return false; }
    }

    // Folders that only an elevated process may write to
    public static bool NeedsAdmin(string dir)
    {
        string full;
        try { full = Norm(dir); } catch { return false; }
        foreach (string v in new[] { "ProgramFiles", "ProgramFiles(x86)", "ProgramW6432" })
        {
            string pf = Environment.GetEnvironmentVariable(v);
            if (!string.IsNullOrEmpty(pf) && full.StartsWith(Norm(pf), StringComparison.OrdinalIgnoreCase)) return true;
        }
        return false;
    }

    // Can this process create files in dir? Folders created only for the test are removed again.
    public static bool CanWrite(string dir, out Exception error)
    {
        error = null;
        string full = null, created = null;
        try
        {
            full = Path.GetFullPath(dir).TrimEnd('\\');
            for (string probe = full; !string.IsNullOrEmpty(probe) && !Directory.Exists(probe); probe = Path.GetDirectoryName(probe)) created = probe;
            Directory.CreateDirectory(full);
            string test = Path.Combine(full, ".pim-write-test-" + Guid.NewGuid().ToString("N").Substring(0, 8) + ".tmp");
            using (FileStream f = new FileStream(test, FileMode.CreateNew, FileAccess.Write, FileShare.None, 4096, FileOptions.DeleteOnClose)) f.WriteByte(0);
            return true;
        }
        catch (Exception e) { error = e; return false; }
        finally
        {
            if (created != null && full != null)
            {
                for (string d = full; !string.IsNullOrEmpty(d); d = Path.GetDirectoryName(d))
                {
                    try { if (Directory.Exists(d)) Directory.Delete(d); } catch { break; }
                    if (string.Equals(d, created, StringComparison.OrdinalIgnoreCase)) break;
                }
            }
        }
    }

    public static string Quote(string s)
    {
        if (s.Length > 0 && s.IndexOfAny(new[] { ' ', '\t', '"' }) < 0) return s;
        StringBuilder sb = new StringBuilder("\"");
        int bs = 0;
        foreach (char c in s)
        {
            if (c == '\\') { bs++; continue; }
            if (c == '"') { sb.Append('\\', bs * 2 + 1); sb.Append('"'); bs = 0; continue; }
            sb.Append('\\', bs); bs = 0; sb.Append(c);
        }
        sb.Append('\\', bs * 2);
        sb.Append('"');
        return sb.ToString();
    }

    public static string JoinArgs(string[] args)
    {
        List<string> parts = new List<string>();
        foreach (string x in args) parts.Add(Quote(x));
        return string.Join(" ", parts.ToArray());
    }

    // Starts this Setup again with the UAC prompt. 0 = started (or exit code when wait), 6 = UAC declined, 7 = failed.
    // PIM_SETUP_NOELEVATE=1 only logs the command (used for testing without a UAC prompt).
    public static int RelaunchElevated(string arguments, bool wait, Action<string> log)
    {
        string exe = Assembly.GetExecutingAssembly().Location;
        if (Environment.GetEnvironmentVariable("PIM_SETUP_NOELEVATE") == "1")
        {
            log("Test mode (PIM_SETUP_NOELEVATE=1), not elevating. Would run: " + Quote(exe) + " " + arguments);
            return 6;
        }
        try
        {
            ProcessStartInfo psi = new ProcessStartInfo(exe, arguments) { UseShellExecute = true, Verb = "runas", WorkingDirectory = Path.GetDirectoryName(exe) };
            using (Process p = Process.Start(psi))
            {
                log("Elevated Setup started" + (p != null ? " (pid " + p.Id + ")" : ""));
                if (!wait || p == null) return 0;
                p.WaitForExit();
                return p.ExitCode;
            }
        }
        catch (System.ComponentModel.Win32Exception e)
        {
            if (e.NativeErrorCode == 1223) { log("Administrator rights were not granted (UAC prompt cancelled)"); return 6; }
            log("Could not restart Setup elevated: " + e.Message);
            return 7;
        }
    }

    // Write/move with 3 attempts and a short pause (antivirus or indexer can hold a new file for a moment)
    static void Retry(string what, string path, Action action, Action<string> log)
    {
        for (int attempt = 1; ; attempt++)
        {
            try { action(); return; }
            catch (Exception e)
            {
                if (!(e is IOException || e is UnauthorizedAccessException)) throw;
                if (attempt >= 3) { log("FAILED to " + what + ": " + path + " - " + e.GetType().Name + ": " + e.Message); throw new SetupWriteException(what, path, e); }
                log("Attempt " + attempt + "/3 to " + what + " failed: " + path + " - " + e.Message + " (retrying)");
                Thread.Sleep(600 * attempt);
            }
        }
    }

    // Processes whose exe lives in the install folder (copies elsewhere, e.g. the original app, are ignored)
    public static List<string> RunningIn(string dir)
    {
        List<string> list = new List<string>();
        string root;
        try { root = Norm(dir); } catch { return list; }
        int self = Process.GetCurrentProcess().Id;
        foreach (Process p in Process.GetProcesses())
        {
            try
            {
                if (p.Id == self) continue;
                string path = p.MainModule.FileName;
                if (path.StartsWith(root, StringComparison.OrdinalIgnoreCase) && !path.EndsWith("\\" + UninstallExe, StringComparison.OrdinalIgnoreCase))
                    list.Add(Path.GetFileName(path) + " (pid " + p.Id + ")");
            }
            catch { }
            finally { p.Dispose(); }
        }
        return list;
    }

    public static bool PortBusy(int port)
    {
        try
        {
            foreach (System.Net.IPEndPoint ep in IPGlobalProperties.GetIPGlobalProperties().GetActiveTcpListeners())
                if (ep.Port == port) return true;
        }
        catch { }
        return false;
    }

    public static void Launch(string dir)
    {
        string exe = Path.Combine(dir, ExeName);
        if (!File.Exists(exe)) return;
        // From an elevated Setup, start the app through Explorer so it runs with normal rights
        if (IsAdmin()) Process.Start(new ProcessStartInfo("explorer.exe", Quote(exe)) { UseShellExecute = true });
        else Process.Start(new ProcessStartInfo(exe) { WorkingDirectory = dir, UseShellExecute = true });
    }

    public static void Install(InstallOptions o, Action<string> log, Action<int, string> progress)
    {
        string dest = Path.GetFullPath(o.Dir).TrimEnd('\\');
        string root = dest + "\\";
        log("Installing to " + dest + (IsAdmin() ? " (administrator)" : " (normal user rights)"));
        Exception werr;
        if (!CanWrite(dest, out werr)) { log("FAILED: folder is not writable: " + dest + " - " + werr.Message); throw new SetupWriteException("write to the install folder", dest, werr); }
        Retry("create the folder", dest, delegate { Directory.CreateDirectory(dest); }, log);
        SortedSet<string> files = new SortedSet<string>(StringComparer.OrdinalIgnoreCase);
        SortedSet<string> extra = new SortedSet<string>(StringComparer.OrdinalIgnoreCase);
        ReadList(dest, files, extra);
        int leftovers = 0;
        try { foreach (string old in Directory.GetFiles(dest, "*.pimtmp", SearchOption.AllDirectories)) { try { File.Delete(old); leftovers++; } catch { } } } catch { }
        if (leftovers > 0) log("Removed " + leftovers + " leftover temporary file(s) from an earlier attempt");

        Assembly asm = Assembly.GetExecutingAssembly();
        using (Stream s = asm.GetManifestResourceStream("payload.zip"))
        {
            if (s == null) throw new InvalidOperationException("This setup contains no payload.");
            using (ZipArchive zip = new ZipArchive(s, ZipArchiveMode.Read))
            {
                List<ZipArchiveEntry> entries = new List<ZipArchiveEntry>();
                long total = 0;
                foreach (ZipArchiveEntry e in zip.Entries)
                {
                    string name = e.FullName.Replace('/', '\\');
                    if (name.EndsWith("\\")) continue;
                    if (!o.Chromium && name.StartsWith("chromium-extension\\", StringComparison.OrdinalIgnoreCase)) continue;
                    entries.Add(e);
                    total += e.Length;
                }
                log("Extracting: " + entries.Count + " files, " + (total / 1048576.0).ToString("0.0") + " MB");
                long done = 0;
                byte[] buf = new byte[1 << 20];
                int lastPct = -1;
                Stopwatch clock = Stopwatch.StartNew();
                foreach (ZipArchiveEntry e in entries)
                {
                    string rel = e.FullName.Replace('/', '\\');
                    string target = Path.GetFullPath(Path.Combine(dest, rel));
                    if (!target.StartsWith(root, StringComparison.OrdinalIgnoreCase)) throw new InvalidOperationException("Invalid path in payload: " + rel);
                    string folder = Path.GetDirectoryName(target);
                    Retry("create the folder", folder, delegate { Directory.CreateDirectory(folder); }, log);
                    string tmp = target + ".pimtmp";
                    long before = done;
                    ZipArchiveEntry entry = e;
                    try
                    {
                        Retry("write", tmp, delegate
                        {
                            done = before;
                            using (Stream input = entry.Open())
                            using (FileStream output = new FileStream(tmp, FileMode.Create, FileAccess.Write, FileShare.None))
                            {
                                int n;
                                while ((n = input.Read(buf, 0, buf.Length)) > 0)
                                {
                                    output.Write(buf, 0, n);
                                    done += n;
                                    int pct = total > 0 ? (int)Math.Min(97, done * 97 / total) : 0;
                                    if (pct != lastPct || clock.ElapsedMilliseconds >= 100)
                                    {
                                        lastPct = pct; clock.Restart();
                                        progress(pct, "Extracting: " + Path.GetFileName(rel));
                                    }
                                }
                            }
                        }, log);
                        Retry("replace", target, delegate
                        {
                            if (File.Exists(target)) { File.SetAttributes(target, FileAttributes.Normal); File.Delete(target); }
                            File.Move(tmp, target);
                        }, log);
                    }
                    catch
                    {
                        try { if (File.Exists(tmp)) File.Delete(tmp); } catch { }
                        throw;
                    }
                    try { File.SetLastWriteTime(target, e.LastWriteTime.DateTime); } catch { }
                    files.Add(rel);
                    log("Placed " + rel + "  (" + Pretty(e.Length) + ")");
                }
            }
        }

        using (Stream u = asm.GetManifestResourceStream(UninstallExe))
        {
            if (u != null)
            {
                string target = Path.Combine(dest, UninstallExe);
                byte[] data;
                using (MemoryStream ms = new MemoryStream()) { u.CopyTo(ms); data = ms.ToArray(); }
                Retry("write", target, delegate { File.WriteAllBytes(target, data); }, log);
                files.Add(UninstallExe);
                log("Placed " + UninstallExe);
            }
        }
        progress(98, "Shortcuts and registration");

        string exe = Path.Combine(dest, ExeName);
        string ico = Path.Combine(dest, IconName);
        if (o.Desktop)
        {
            string lnk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.DesktopDirectory), AppName + ".lnk");
            if (Shortcut(lnk, exe, dest, ico, log)) { extra.Add("S|" + lnk); log("Desktop shortcut: " + lnk); }
        }
        if (o.StartMenu)
        {
            string lnk = Path.Combine(Environment.GetFolderPath(Environment.SpecialFolder.Programs), AppName + ".lnk");
            if (Shortcut(lnk, exe, dest, ico, log)) { extra.Add("S|" + lnk); log("Start menu shortcut: " + lnk); }
        }
        if (o.Registry)
        {
            using (RegistryKey run = Registry.CurrentUser.CreateSubKey(RunKey))
            {
                if (o.Autostart) { run.SetValue(RunValue, "\"" + exe + "\" --minimized"); log("Start with Windows: on"); }
                else
                {
                    string cur = run.GetValue(RunValue) as string;
                    if (cur != null && cur.IndexOf(root, StringComparison.OrdinalIgnoreCase) >= 0) { run.DeleteValue(RunValue, false); log("Start with Windows: off"); }
                }
            }
            long kb = 0;
            foreach (string f in files) { try { kb += new FileInfo(Path.Combine(dest, f)).Length / 1024; } catch { } }
            using (RegistryKey k = Registry.CurrentUser.CreateSubKey(RegKey))
            {
                k.SetValue("DisplayName", AppName);
                k.SetValue("DisplayIcon", File.Exists(ico) ? ico : exe + ",0");
                k.SetValue("DisplayVersion", Version);
                k.SetValue("Publisher", Publisher);
                k.SetValue("InstallLocation", dest);
                k.SetValue("UninstallString", "\"" + Path.Combine(dest, UninstallExe) + "\"");
                k.SetValue("QuietUninstallString", "\"" + Path.Combine(dest, UninstallExe) + "\" /silent");
                k.SetValue("EstimatedSize", (int)Math.Min(int.MaxValue, kb), RegistryValueKind.DWord);
                k.SetValue("NoModify", 1, RegistryValueKind.DWord);
                k.SetValue("NoRepair", 1, RegistryValueKind.DWord);
                k.SetValue("InstallDate", DateTime.Now.ToString("yyyyMMdd"));
            }
            log("Registered in Installed apps (current user only)");
        }
        Retry("write", Path.Combine(dest, ListFile), delegate { WriteList(dest, files, extra); }, log);
        progress(100, "Installation complete");
        log("Done: " + dest);
    }

    static string Pretty(long b)
    {
        if (b >= 1048576) return (b / 1048576.0).ToString("0.0") + " MB";
        if (b >= 1024) return (b / 1024.0).ToString("0") + " KB";
        return b + " B";
    }

    static void ReadList(string dir, ISet<string> files, ISet<string> extra)
    {
        string p = Path.Combine(dir, ListFile);
        if (!File.Exists(p)) return;
        foreach (string line in File.ReadAllLines(p))
        {
            if (line.StartsWith("F|")) files.Add(line.Substring(2));
            else if (line.StartsWith("S|")) extra.Add(line);
        }
    }

    static void WriteList(string dir, IEnumerable<string> files, IEnumerable<string> extra)
    {
        StringBuilder sb = new StringBuilder();
        sb.AppendLine("# Pro Internet Manager - files placed by Setup (used by Uninstall.exe)");
        foreach (string f in files) sb.AppendLine("F|" + f);
        foreach (string x in extra) sb.AppendLine(x);
        File.WriteAllText(Path.Combine(dir, ListFile), sb.ToString(), new UTF8Encoding(false));
    }

    static bool Shortcut(string link, string target, string dir, string icon, Action<string> log)
    {
        try
        {
            Directory.CreateDirectory(Path.GetDirectoryName(link));
            Type shellType = Type.GetTypeFromProgID("WScript.Shell");
            object shell = Activator.CreateInstance(shellType);
            object sc = shellType.InvokeMember("CreateShortcut", BindingFlags.InvokeMethod, null, shell, new object[] { link });
            Type t = sc.GetType();
            t.InvokeMember("TargetPath", BindingFlags.SetProperty, null, sc, new object[] { target });
            t.InvokeMember("WorkingDirectory", BindingFlags.SetProperty, null, sc, new object[] { dir });
            t.InvokeMember("Description", BindingFlags.SetProperty, null, sc, new object[] { "Pro Internet Manager by Triwar Design" });
            if (File.Exists(icon)) t.InvokeMember("IconLocation", BindingFlags.SetProperty, null, sc, new object[] { icon });
            t.InvokeMember("Save", BindingFlags.InvokeMethod, null, sc, null);
            Marshal.FinalReleaseComObject(sc);
            Marshal.FinalReleaseComObject(shell);
            return true;
        }
        catch (Exception e) { log("Shortcut failed: " + e.Message); return false; }
    }

    // Removes only what Setup placed: listed files, shortcuts, own Run value, own Uninstall entry.
    public static void Uninstall(string dir, bool purge, Action<string> log, Action<int, string> progress)
    {
        dir = Path.GetFullPath(dir).TrimEnd('\\');
        string root = dir + "\\";
        SortedSet<string> files = new SortedSet<string>(StringComparer.OrdinalIgnoreCase);
        SortedSet<string> extra = new SortedSet<string>(StringComparer.OrdinalIgnoreCase);
        if (!File.Exists(Path.Combine(dir, ListFile)))
            throw new InvalidOperationException("No installation found in " + dir + " (uninstall.dat is missing).");
        ReadList(dir, files, extra);
        log("Removing from " + dir);
        string self = Assembly.GetExecutingAssembly().Location;
        int i = 0, count = Math.Max(1, files.Count);
        foreach (string rel in files)
        {
            i++;
            string p = Path.GetFullPath(Path.Combine(dir, rel));
            if (!p.StartsWith(root, StringComparison.OrdinalIgnoreCase)) continue;
            if (string.Equals(p, self, StringComparison.OrdinalIgnoreCase)) continue;
            try { if (File.Exists(p)) { File.SetAttributes(p, FileAttributes.Normal); File.Delete(p); log("Removed " + rel); } }
            catch (Exception e) { log("Could not remove: " + rel + " (" + e.Message + ")"); }
            progress(i * 85 / count, "Removing: " + Path.GetFileName(rel));
        }
        foreach (string x in extra)
        {
            string lnk = x.Substring(2);
            try { if (File.Exists(lnk)) { File.Delete(lnk); log("Shortcut removed: " + lnk); } } catch (Exception e) { log("Shortcut: " + e.Message); }
        }
        try
        {
            using (RegistryKey run = Registry.CurrentUser.OpenSubKey(RunKey, true))
            {
                string cur = run == null ? null : run.GetValue(RunValue) as string;
                if (cur != null && cur.IndexOf(root, StringComparison.OrdinalIgnoreCase) >= 0) { run.DeleteValue(RunValue, false); log("Start with Windows removed"); }
            }
            bool ours = false;
            using (RegistryKey k = Registry.CurrentUser.OpenSubKey(RegKey))
            {
                string loc = k == null ? null : k.GetValue("InstallLocation") as string;
                ours = loc != null && string.Equals(loc.TrimEnd('\\'), dir, StringComparison.OrdinalIgnoreCase);
            }
            if (ours) { Registry.CurrentUser.DeleteSubKeyTree(RegKey, false); log("Removed from Installed apps"); }
        }
        catch (Exception e) { log("Registry: " + e.Message); }
        if (purge)
        {
            string sd = SettingsDir();
            try { if (Directory.Exists(sd)) { Directory.Delete(sd, true); log("Settings removed: " + sd); } }
            catch (Exception e) { log("Settings: " + e.Message); }
            string ld = LocalDataDir();
            try { if (Directory.Exists(ld)) { Directory.Delete(ld, true); log("Update files removed: " + ld); } }
            catch (Exception e) { log("Update files: " + e.Message); }
        }
        try { File.Delete(Path.Combine(dir, ListFile)); } catch { }
        if (Directory.Exists(dir))
        {
            List<string> dirs = new List<string>(Directory.GetDirectories(dir, "*", SearchOption.AllDirectories));
            dirs.Sort(delegate(string a, string b) { return b.Length.CompareTo(a.Length); });
            foreach (string d in dirs) { try { if (Directory.GetFileSystemEntries(d).Length == 0) Directory.Delete(d); } catch { } }
        }
        progress(95, "Finishing");
        if (self.StartsWith(root, StringComparison.OrdinalIgnoreCase))
        {
            // Uninstall.exe cannot delete itself while running: a hidden cmd does it after we exit.
            string cmd = "/c ping 127.0.0.1 -n 3 > nul & del /f /q \"" + self + "\" & rmdir \"" + dir + "\"";
            Process.Start(new ProcessStartInfo("cmd.exe", cmd) { CreateNoWindow = true, UseShellExecute = false, WindowStyle = ProcessWindowStyle.Hidden });
            log("Uninstall.exe and the empty folder will be removed after exit");
        }
        else
        {
            try { string u = Path.Combine(dir, UninstallExe); if (File.Exists(u)) File.Delete(u); } catch { }
            try { if (Directory.Exists(dir) && Directory.GetFileSystemEntries(dir).Length == 0) Directory.Delete(dir); } catch { }
        }
        progress(100, "Uninstall complete");
        log("Done");
    }

    public static string FindBrowser(string exe, params string[] candidates)
    {
        foreach (RegistryKey hive in new[] { Registry.CurrentUser, Registry.LocalMachine })
        {
            try
            {
                using (RegistryKey k = hive.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\App Paths\" + exe))
                {
                    string p = k == null ? null : k.GetValue(null) as string;
                    if (!string.IsNullOrEmpty(p)) { p = p.Trim('"'); if (File.Exists(p)) return p; }
                }
            }
            catch { }
        }
        foreach (string c in candidates)
        {
            string p = Environment.ExpandEnvironmentVariables(c);
            if (File.Exists(p)) return p;
        }
        return null;
    }

    public static string Firefox() { return FindBrowser("firefox.exe", @"%ProgramFiles%\Mozilla Firefox\firefox.exe", @"%ProgramFiles(x86)%\Mozilla Firefox\firefox.exe", @"%LOCALAPPDATA%\Mozilla Firefox\firefox.exe"); }
    public static string Chrome() { return FindBrowser("chrome.exe", @"%ProgramFiles%\Google\Chrome\Application\chrome.exe", @"%ProgramFiles(x86)%\Google\Chrome\Application\chrome.exe", @"%LOCALAPPDATA%\Google\Chrome\Application\chrome.exe"); }
    public static string Edge() { return FindBrowser("msedge.exe", @"%ProgramFiles(x86)%\Microsoft\Edge\Application\msedge.exe", @"%ProgramFiles%\Microsoft\Edge\Application\msedge.exe"); }
    public static string Brave() { return FindBrowser("brave.exe", @"%ProgramFiles%\BraveSoftware\Brave-Browser\Application\brave.exe", @"%ProgramFiles(x86)%\BraveSoftware\Brave-Browser\Application\brave.exe", @"%LOCALAPPDATA%\BraveSoftware\Brave-Browser\Application\brave.exe"); }
}

// ---------------------------------------------------------------- theme (Universal Upscaler installer look)
internal static class Theme
{
    public static readonly Color Bg = Color.FromArgb(9, 12, 9);
    public static readonly Color CardFill = Color.FromArgb(225, 17, 21, 17);
    public static readonly Color CardBorder = Color.FromArgb(48, 56, 48);
    public static readonly Color Accent = Color.FromArgb(118, 185, 0);        // #76B900
    public static readonly Color AccentHover = Color.FromArgb(136, 206, 18);
    public static readonly Color AccentText = Color.FromArgb(10, 14, 6);
    public static readonly Color Text = Color.FromArgb(242, 245, 240);
    public static readonly Color Dim = Color.FromArgb(150, 158, 148);
    public static readonly Color Green = Color.FromArgb(132, 196, 20);
    public static readonly Color Btn = Color.FromArgb(30, 35, 30);
    public static readonly Color BtnHover = Color.FromArgb(44, 51, 44);
    public static readonly Color BtnBorder = Color.FromArgb(64, 72, 64);
    public static readonly Color Field = Color.FromArgb(13, 16, 13);
    public static float K = 1f;

    public static int S(float v) { return (int)Math.Round(v * K); }

    public static GraphicsPath Round(RectangleF r, float radius)
    {
        GraphicsPath p = new GraphicsPath();
        float d = Math.Min(radius * 2f, Math.Min(r.Width, r.Height));
        if (d <= 0.5f) { p.AddRectangle(r); return p; }
        p.AddArc(r.X, r.Y, d, d, 180, 90);
        p.AddArc(r.Right - d, r.Y, d, d, 270, 90);
        p.AddArc(r.Right - d, r.Bottom - d, d, d, 0, 90);
        p.AddArc(r.X, r.Bottom - d, d, d, 90, 90);
        p.CloseFigure();
        return p;
    }
}

internal class TControl : Control
{
    public TControl()
    {
        SetStyle(ControlStyles.SupportsTransparentBackColor | ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
        BackColor = Color.Transparent;
    }
}

internal class ClearPanel : Panel
{
    public ClearPanel()
    {
        SetStyle(ControlStyles.SupportsTransparentBackColor | ControlStyles.UserPaint | ControlStyles.AllPaintingInWmPaint | ControlStyles.OptimizedDoubleBuffer | ControlStyles.ResizeRedraw, true);
        BackColor = Color.Transparent;
    }
}

// Rounded card; green 2px border when selected
internal class Card : ClearPanel
{
    private bool selected;
    public Color Fill = Theme.CardFill;
    public bool Selected { get { return selected; } set { if (selected != value) { selected = value; Invalidate(); } } }

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        float w = selected ? Math.Max(2f, Theme.K * 1.6f) : 1f;
        float i = w / 2f;
        RectangleF r = new RectangleF(i, i, Width - w - 1f + 1f, Height - w - 1f + 1f);
        r.Width = Width - w; r.Height = Height - w;
        using (GraphicsPath p = Theme.Round(r, Theme.S(10)))
        {
            using (SolidBrush b = new SolidBrush(Fill)) g.FillPath(b, p);
            using (Pen pen = new Pen(selected ? Theme.Accent : Theme.CardBorder, w)) g.DrawPath(pen, p);
        }
        base.OnPaint(e);
    }
}

internal class RoundButton : TControl, IButtonControl
{
    public bool Primary;
    private bool hover;

    public RoundButton(string text, bool primary)
    {
        Text = text;
        Primary = primary;
        Cursor = Cursors.Hand;
        Font = new Font("Segoe UI Semibold", 10.5f);
        TabStop = true;
    }

    public DialogResult DialogResult { get; set; }
    public void NotifyDefault(bool value) { }
    public void PerformClick() { if (Enabled && Visible) OnClick(EventArgs.Empty); }

    protected override void OnMouseEnter(EventArgs e) { hover = true; Invalidate(); base.OnMouseEnter(e); }
    protected override void OnMouseLeave(EventArgs e) { hover = false; Invalidate(); base.OnMouseLeave(e); }
    protected override void OnEnabledChanged(EventArgs e) { Invalidate(); base.OnEnabledChanged(e); }
    protected override void OnTextChanged(EventArgs e) { Invalidate(); base.OnTextChanged(e); }
    protected override void OnKeyUp(KeyEventArgs e)
    {
        if (e.KeyCode == Keys.Space) PerformClick();
        base.OnKeyUp(e);
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        Color fill = Primary ? (hover ? Theme.AccentHover : Theme.Accent) : (hover ? Theme.BtnHover : Theme.Btn);
        if (!Enabled) fill = Primary ? Color.FromArgb(58, 78, 28) : Color.FromArgb(24, 28, 24);
        RectangleF r = new RectangleF(0.5f, 0.5f, Width - 1.5f, Height - 1.5f);
        using (GraphicsPath p = Theme.Round(r, Theme.S(8)))
        {
            using (SolidBrush b = new SolidBrush(fill)) g.FillPath(b, p);
            if (!Primary) using (Pen pen = new Pen(Theme.BtnBorder)) g.DrawPath(pen, p);
        }
        Color tc = Primary ? Theme.AccentText : Theme.Text;
        if (!Enabled) tc = Theme.Dim;
        TextRenderer.DrawText(g, Text, Font, ClientRectangle, tc, TextFormatFlags.HorizontalCenter | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine | TextFormatFlags.EndEllipsis);
    }
}

// Round radio button or rounded checkbox with its caption
internal class ToggleOption : TControl
{
    public bool Radio;
    private bool chk;
    public event EventHandler CheckedChanged;

    public ToggleOption(string name, string text, bool radio)
    {
        Name = name;
        Text = text;
        Radio = radio;
        Cursor = Cursors.Hand;
        Font = new Font("Segoe UI Semibold", 11f);
    }

    public bool Checked
    {
        get { return chk; }
        set
        {
            if (chk == value) return;
            chk = value;
            Invalidate();
            if (CheckedChanged != null) CheckedChanged(this, EventArgs.Empty);
        }
    }

    protected override void OnClick(EventArgs e)
    {
        if (!Enabled) return;
        if (Radio) Checked = true; else Checked = !Checked;
        base.OnClick(e);
    }

    protected override void OnEnabledChanged(EventArgs e) { Invalidate(); base.OnEnabledChanged(e); }

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        float d = Theme.S(22);
        float y = (Height - d) / 2f;
        RectangleF box = new RectangleF(Theme.K * 1.5f, y, d, d);
        Color ring = chk ? Theme.Accent : Color.FromArgb(150, 158, 148);
        if (!Enabled) ring = chk ? Color.FromArgb(96, 140, 20) : Color.FromArgb(80, 86, 80);
        float pw = Math.Max(1.5f, Theme.K * 1.6f);
        if (Radio)
        {
            using (Pen p = new Pen(ring, pw)) g.DrawEllipse(p, box);
            if (chk)
            {
                float dd = d * 0.5f;
                using (SolidBrush b = new SolidBrush(ring)) g.FillEllipse(b, box.X + (d - dd) / 2f, box.Y + (d - dd) / 2f, dd, dd);
            }
        }
        else
        {
            using (GraphicsPath path = Theme.Round(box, Theme.S(5)))
            {
                if (chk)
                {
                    using (SolidBrush b = new SolidBrush(ring)) g.FillPath(b, path);
                    using (Pen m = new Pen(Theme.AccentText, Math.Max(2f, Theme.K * 2.2f)))
                    {
                        m.StartCap = LineCap.Round; m.EndCap = LineCap.Round; m.LineJoin = LineJoin.Round;
                        g.DrawLines(m, new PointF[] {
                            new PointF(box.X + d * 0.25f, box.Y + d * 0.52f),
                            new PointF(box.X + d * 0.43f, box.Y + d * 0.70f),
                            new PointF(box.X + d * 0.76f, box.Y + d * 0.32f) });
                    }
                }
                else using (Pen p = new Pen(ring, pw)) g.DrawPath(p, path);
            }
        }
        Rectangle tr = new Rectangle((int)(box.Right + Theme.S(14)), 0, Math.Max(1, Width - (int)(box.Right + Theme.S(14))), Height);
        TextRenderer.DrawText(g, Text, Font, tr, Theme.Text, TextFormatFlags.Left | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine | TextFormatFlags.EndEllipsis);
    }
}

internal class RoundProgress : TControl
{
    private int val;
    public int Value { get { return val; } set { int v = Math.Max(0, Math.Min(100, value)); if (v != val) { val = v; Invalidate(); } } }

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        RectangleF r = new RectangleF(0, 0, Width - 1, Height - 1);
        float rad = Height / 2f;
        using (GraphicsPath p = Theme.Round(r, rad)) using (SolidBrush b = new SolidBrush(Color.FromArgb(34, 40, 34))) g.FillPath(b, p);
        if (val > 0)
        {
            float w = Math.Max(Height, (Width - 1) * val / 100f);
            using (GraphicsPath p = Theme.Round(new RectangleF(0, 0, w, Height - 1), rad)) using (SolidBrush b = new SolidBrush(Theme.Accent)) g.FillPath(b, p);
        }
    }
}

// Rounded text field (TextBox without border inside a rounded card)
internal class FieldBox : Card
{
    public TextBox Box;
    private bool multi;

    [DllImport("uxtheme.dll", CharSet = CharSet.Unicode)]
    private static extern int SetWindowTheme(IntPtr hwnd, string app, string idList);

    public FieldBox(string name, bool multiline, Font font)
    {
        multi = multiline;
        Fill = Theme.Field;
        Box = new TextBox
        {
            Name = name,
            BorderStyle = BorderStyle.None,
            BackColor = Theme.Field,
            ForeColor = Theme.Text,
            Font = font,
            Multiline = multiline
        };
        if (multiline) Box.HandleCreated += delegate { try { SetWindowTheme(Box.Handle, "DarkMode_Explorer", null); } catch { } };
        Box.Enter += delegate { Selected = true; };
        Box.Leave += delegate { Selected = false; };
        Controls.Add(Box);
        Cursor = Cursors.IBeam;
        Click += delegate { Box.Focus(); };
    }

    protected override void OnLayout(LayoutEventArgs e)
    {
        base.OnLayout(e);
        if (Box == null) return;
        int pad = Theme.S(multi ? 10 : 12);
        if (multi) Box.SetBounds(pad, Theme.S(8), Width - pad * 2, Height - Theme.S(16));
        else Box.SetBounds(pad, (Height - Box.PreferredHeight) / 2, Width - pad * 2, Box.PreferredHeight);
    }
}

// ---------------------------------------------------------------- shared window chrome (backdrop, header, assets)
internal class ThemedForm : Form
{
    protected Image background;
    protected Bitmap logo;
    private Bitmap backdrop;
    protected string HeaderTitle = Engine.AppName;
    protected int CW = 780;

    [DllImport("dwmapi.dll")]
    private static extern int DwmSetWindowAttribute(IntPtr hwnd, int attr, ref int value, int size);

    protected static int S(float v) { return Theme.S(v); }
    protected static Point P(int x, int y) { return new Point(S(x), S(y)); }
    protected static Size Sz(int w, int h) { return new Size(S(w), S(h)); }

    public ThemedForm()
    {
        using (Graphics g = Graphics.FromHwnd(IntPtr.Zero)) Theme.K = Math.Max(1f, g.DpiX / 96f);
        FormBorderStyle = FormBorderStyle.FixedSingle;
        MaximizeBox = false;
        StartPosition = FormStartPosition.CenterScreen;
        BackColor = Theme.Bg;
        ForeColor = Theme.Text;
        Font = new Font("Segoe UI", 9.5f);
        AutoScaleMode = AutoScaleMode.None;
        DoubleBuffered = true;
        LoadAssets();
    }

    protected override CreateParams CreateParams
    {
        get { CreateParams cp = base.CreateParams; cp.ExStyle |= 0x02000000; return cp; }
    }

    protected override void OnHandleCreated(EventArgs e)
    {
        base.OnHandleCreated(e);
        try
        {
            int on = 1; DwmSetWindowAttribute(Handle, 20, ref on, 4);
            int round = 2; DwmSetWindowAttribute(Handle, 33, ref round, 4);
            int border = ColorRef(Color.FromArgb(58, 86, 18)); DwmSetWindowAttribute(Handle, 34, ref border, 4);
            int caption = ColorRef(Color.FromArgb(10, 13, 10)); DwmSetWindowAttribute(Handle, 35, ref caption, 4);
            int text = ColorRef(Color.FromArgb(200, 206, 198)); DwmSetWindowAttribute(Handle, 36, ref text, 4);
        }
        catch { }
    }

    private static int ColorRef(Color c) { return c.R | (c.G << 8) | (c.B << 16); }

    public static byte[] Resource(string name)
    {
        using (Stream s = Assembly.GetExecutingAssembly().GetManifestResourceStream(name))
        {
            if (s == null) return null;
            using (MemoryStream ms = new MemoryStream()) { s.CopyTo(ms); return ms.ToArray(); }
        }
    }

    private void LoadAssets()
    {
        try
        {
            byte[] data = Resource("bg_upscaler.jpg");
            if (data != null) using (MemoryStream ms = new MemoryStream(data)) using (Image img = Image.FromStream(ms)) background = new Bitmap(img);
        }
        catch { }
        try
        {
            byte[] data = Resource(Engine.IconName);
            if (data != null)
            {
                using (MemoryStream ms = new MemoryStream(data)) Icon = new Icon(ms);
                logo = IcoPng(data);
                if (logo == null) using (MemoryStream ms = new MemoryStream(data)) using (Icon big = new Icon(ms, 256, 256)) logo = big.ToBitmap();
            }
        }
        catch { }
    }

    private static Bitmap IcoPng(byte[] ico)
    {
        try
        {
            int count = BitConverter.ToUInt16(ico, 4);
            int best = -1, bestSize = 0;
            for (int i = 0; i < count; i++)
            {
                int e = 6 + i * 16;
                int w = ico[e] == 0 ? 256 : ico[e];
                int off = BitConverter.ToInt32(ico, e + 12);
                bool png = off + 8 <= ico.Length && ico[off] == 0x89 && ico[off + 1] == 0x50 && ico[off + 2] == 0x4E && ico[off + 3] == 0x47;
                if (png && w > bestSize) { bestSize = w; best = e; }
            }
            if (best < 0) return null;
            int len = BitConverter.ToInt32(ico, best + 8);
            int offset = BitConverter.ToInt32(ico, best + 12);
            using (MemoryStream ms = new MemoryStream(ico, offset, len)) using (Image img = Image.FromStream(ms)) return new Bitmap(img);
        }
        catch { return null; }
    }

    private void EnsureBackdrop()
    {
        if (backdrop != null && backdrop.Width == ClientSize.Width && backdrop.Height == ClientSize.Height) return;
        if (ClientSize.Width < 1 || ClientSize.Height < 1) return;
        if (backdrop != null) backdrop.Dispose();
        int w = ClientSize.Width, h = ClientSize.Height;
        backdrop = new Bitmap(w, h);
        using (Graphics g = Graphics.FromImage(backdrop))
        {
            g.SmoothingMode = SmoothingMode.AntiAlias;
            g.InterpolationMode = InterpolationMode.HighQualityBicubic;
            g.Clear(Theme.Bg);
            if (background != null)
            {
                float sc = Math.Max(w / (float)background.Width, h / (float)background.Height);
                float bw = background.Width * sc, bh = background.Height * sc;
                g.DrawImage(background, (w - bw) / 2f, (h - bh) / 2f, bw, bh);
                using (LinearGradientBrush shade = new LinearGradientBrush(new Rectangle(0, 0, w, h), Color.FromArgb(232, 7, 10, 7), Color.FromArgb(168, 7, 10, 7), 90f)) g.FillRectangle(shade, 0, 0, w, h);
            }
            else using (LinearGradientBrush b = new LinearGradientBrush(new Rectangle(0, 0, w, h), Theme.Bg, Color.FromArgb(16, 30, 14), 90f)) g.FillRectangle(b, 0, 0, w, h);

            Rectangle iconRect = new Rectangle(S(40), S(32), S(76), S(76));
            if (logo != null) g.DrawImage(logo, iconRect);
            using (Font title = new Font("Segoe UI", 20f, FontStyle.Bold))
                TextRenderer.DrawText(g, HeaderTitle, title, new Point(S(130), S(34)), Theme.Text, TextFormatFlags.NoPadding);
            using (Font tag = new Font("Segoe UI Semibold", 11f))
                TextRenderer.DrawText(g, "by Triwar Design", tag, new Point(S(133), S(80)), Color.FromArgb(0x76, 0xB9, 0x00), TextFormatFlags.NoPadding);
            Rectangle rule = new Rectangle(S(40), S(128), S(CW), Math.Max(1, S(1)));
            using (LinearGradientBrush b = new LinearGradientBrush(new Rectangle(rule.X - 1, rule.Y, rule.Width + 2, rule.Height), Color.FromArgb(170, 118, 185, 0), Color.FromArgb(25, 118, 185, 0), 0f))
                g.FillRectangle(b, rule);
        }
    }

    protected override void OnPaintBackground(PaintEventArgs e)
    {
        EnsureBackdrop();
        if (backdrop != null) e.Graphics.DrawImageUnscaled(backdrop, 0, 0);
        else e.Graphics.Clear(Theme.Bg);
    }

    protected Label Lbl(string text, int x, int y, int w, int h, float size, Color color, bool bold)
    {
        return new Label
        {
            Text = text, Location = P(x, y), Size = Sz(w, h), ForeColor = color,
            Font = new Font(bold ? "Segoe UI Semibold" : "Segoe UI", size, FontStyle.Regular),
            BackColor = Color.Transparent, UseMnemonic = false
        };
    }

    protected LinkLabel Link(string text, int x, int y, int w, int h, string url)
    {
        LinkLabel l = new LinkLabel
        {
            Text = text, Location = P(x, y), Size = Sz(w, h), BackColor = Color.Transparent,
            Font = new Font("Segoe UI", 9f), LinkColor = Theme.Green, ActiveLinkColor = Theme.AccentHover, VisitedLinkColor = Theme.Green,
            LinkBehavior = LinkBehavior.HoverUnderline, AutoEllipsis = true, UseMnemonic = false
        };
        l.LinkClicked += delegate { try { Process.Start(new ProcessStartInfo(url) { UseShellExecute = true }); } catch { } };
        return l;
    }

    protected Card CardAt(int x, int y, int w, int h) { return new Card { Location = P(x, y), Size = Sz(w, h) }; }

    protected void WireClick(Control root, EventHandler handler, Control skip)
    {
        root.Click += handler;
        root.Cursor = Cursors.Hand;
        foreach (Control child in root.Controls)
        {
            if (child == skip || child is TextBox || child is RoundButton || child is FieldBox || child is LinkLabel) continue;
            WireClick(child, handler, skip);
        }
    }
}

// ---------------------------------------------------------------- setup / uninstall window
internal sealed class SetupForm : ThemedForm
{
    private const int W = 860;
    private const int H = 660;

    private readonly bool uninstallMode;
    private readonly string defaultPath;
    private string[] steps;
    private string[] subs;
    private Panel[] pages;
    private Label pageTitle, pageSub;
    private RoundButton back, next, cancel;
    private TextBox location;
    private ToggleOption optDefault, optCustom;
    private Card cardDefault, cardCustom;
    private Label freeDefault, freeCustom;
    private bool settingPath;
    private ToggleOption optDesktop, optStart, optAuto, optChromium, optLaunch, optPurge;
    private RoundProgress progress;
    private Label progressText, progressPath, progressPercent, progressEta;
    private TextBox logBox;
    private RoundButton chromiumBtn;
    private int page;
    private bool busy;
    private DateTime progressStart = DateTime.MinValue;
    private readonly Args startArgs;

    private readonly Queue<string> logPending = new Queue<string>();
    private readonly object logLock = new object();
    private System.Windows.Forms.Timer logTimer;
    private int logLines;
    private const int LogMaxLines = 500;
    private const int LogTrimTo = 350;

    public SetupForm(bool uninstall, string dir) : this(uninstall, new Args { Dir = dir }) { }

    public SetupForm(bool uninstall, Args a)
    {
        startArgs = a;
        string dir = a.Dir;
        uninstallMode = uninstall;
        defaultPath = uninstall ? (dir ?? Engine.SelfDir()) : (dir ?? Engine.DefaultDir());
        Text = uninstall ? "Uninstall Pro Internet Manager" : "Pro Internet Manager Setup";
        ClientSize = Sz(W, H);
        logTimer = new System.Windows.Forms.Timer { Interval = 50 };
        logTimer.Tick += delegate { FlushLog(); };
        logTimer.Start();
        FormClosed += delegate { logTimer.Stop(); };
        FormClosing += delegate(object s, FormClosingEventArgs e) { if (busy) e.Cancel = true; };

        pageTitle = new Label { ForeColor = Theme.Text, Font = new Font("Segoe UI Semibold", 13f), BackColor = Color.Transparent, Location = P(40, 146), Size = Sz(CW, 30) };
        pageSub = new Label { ForeColor = Theme.Dim, Font = new Font("Segoe UI", 10f), BackColor = Color.Transparent, Location = P(40, 178), Size = Sz(CW, 26) };
        Controls.Add(pageTitle);
        Controls.Add(pageSub);

        if (uninstall)
        {
            steps = new[] { "Uninstall", "Working", "Done" };
            subs = new[] { "Only the files from this installation are removed.", "Removing files, shortcuts and registration.", "Pro Internet Manager has been removed from this computer." };
            pages = new Panel[] { UninstallPage(), ProgressPage(), FinishPage() };
        }
        else
        {
            steps = new[] { "Welcome", "Install location", "Options", "Installing", "Finished" };
            subs = new[] { "A fast download manager with browser bridge, YouTube and HLS.", "Choose where to install Pro Internet Manager. Program Files asks for administrator rights once.", "Shortcuts, startup and browser extensions.", "Files are being extracted and placed.", "Everything is ready." };
            pages = new Panel[] { WelcomePage(), LocationPage(), OptionsPage(), ProgressPage(), FinishPage() };
        }
        foreach (Panel p in pages)
        {
            p.Location = P(40, 216);
            p.Size = Sz(CW, 352);
            p.Visible = false;
            Controls.Add(p);
        }

        next = new RoundButton("Next", true) { Size = Sz(230, 46), Location = P(W - 40 - 230, H - 28 - 46) };
        next.Click += delegate { GoNext(); };
        back = new RoundButton("Back", false) { Size = Sz(150, 46), Location = P(W - 40 - 230 - 14 - 150, H - 28 - 46) };
        back.Click += delegate { GoBack(); };
        cancel = new RoundButton("Cancel", false) { Size = Sz(150, 46), Location = P(W - 40 - 230 - 14 - 150, H - 28 - 46), Visible = false };
        cancel.Click += delegate { Close(); };
        Controls.Add(next);
        Controls.Add(back);
        Controls.Add(cancel);
        AcceptButton = next;
        ShowPage(0);

        // Restarted elevated (UAC): take over the options from the first window and start right away
        if (a.AutoInstall)
        {
            if (uninstall) optPurge.Checked = a.Purge;
            else
            {
                optDesktop.Checked = !a.NoShortcuts && !a.NoDesktop;
                optStart.Checked = !a.NoShortcuts && !a.NoStartMenu;
                optAuto.Checked = a.Autostart;
                optChromium.Checked = !a.NoChromium;
                optLaunch.Checked = a.Run;
            }
            Shown += delegate { BeginInvoke((MethodInvoker)delegate { StartWork(); }); };
        }
    }

    private string ElevatedArgs(string dir)
    {
        StringBuilder sb = new StringBuilder("/elevated /autoinstall ");
        if (uninstallMode) { sb.Append("/uninstall "); if (optPurge.Checked) sb.Append("/purge "); }
        else
        {
            if (!optDesktop.Checked) sb.Append("/nodesktop ");
            if (!optStart.Checked) sb.Append("/nostartmenu ");
            if (optAuto.Checked) sb.Append("/autostart ");
            if (!optChromium.Checked) sb.Append("/nochromium ");
            if (optLaunch.Checked) sb.Append("/run ");
        }
        sb.Append(Engine.Quote("/dir=" + dir));
        return sb.ToString();
    }

    // Checks the folder before anything is copied. Under Program Files: restart elevated (UAC).
    // Returns true when this window may continue with dir.
    private bool EnsureWritable(string dir)
    {
        Exception werr;
        if (Engine.CanWrite(dir, out werr)) return true;
        Log("Folder is not writable: " + dir + " - " + werr.Message);
        if (Engine.NeedsAdmin(dir) && !Engine.IsAdmin() && !startArgs.Elevated)
        {
            Log("Restarting Setup with administrator rights (UAC) for " + dir);
            int r = Engine.RelaunchElevated(ElevatedArgs(dir), false, Log);
            if (r == 0) { Close(); return false; }
        }
        string std = Engine.UserDir();
        if (uninstallMode)
        {
            MessageBox.Show(this, "Setup cannot remove files from this folder:\n" + dir + "\n\nReason: " + werr.Message + "\n\nRun Uninstall as administrator.\n\nLog: " + SetupLog.FilePath, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning);
            return false;
        }
        if (string.Equals(dir.TrimEnd('\\'), std, StringComparison.OrdinalIgnoreCase))
        {
            MessageBox.Show(this, "Setup cannot write to this folder:\n" + dir + "\n\nReason: " + werr.Message + "\n\nLog: " + SetupLog.FilePath, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Error);
            ShowPage(1);
            return false;
        }
        DialogResult answer = MessageBox.Show(this, "Setup cannot write to this folder:\n" + dir + "\n\nReason: " + werr.Message + "\n\nInstall to your user folder instead? No admin rights needed:\n" + std + "\n\nLog: " + SetupLog.FilePath,
            Engine.AppName, MessageBoxButtons.YesNo, MessageBoxIcon.Warning);
        if (answer == DialogResult.Yes) { location.Text = std; optCustom.Checked = true; Log("Switched to the user folder: " + std); return EnsureWritable(std); }
        ShowPage(1);
        return false;
    }

    protected override void OnPaint(PaintEventArgs e)
    {
        Graphics g = e.Graphics;
        g.SmoothingMode = SmoothingMode.AntiAlias;
        using (Font f = new Font("Segoe UI", 9.5f))
            TextRenderer.DrawText(g, "Step " + (page + 1) + " of " + steps.Length + "  \u00B7  " + steps[page], f, new Rectangle(S(420), S(44), S(400), S(24)), Theme.Dim, TextFormatFlags.Right | TextFormatFlags.VerticalCenter | TextFormatFlags.SingleLine);
        float x = S(40);
        float y = S(H - 28 - 46 + 12);
        for (int i = 0; i < steps.Length; i++)
        {
            float pw = i == page ? S(24) : S(7);
            Color c = i == page ? Theme.Accent : (i < page ? Color.FromArgb(70, 104, 20) : Color.FromArgb(58, 66, 58));
            using (GraphicsPath p = Theme.Round(new RectangleF(x, y, pw, S(7)), S(3.5f))) using (SolidBrush b = new SolidBrush(c)) g.FillPath(b, p);
            x += pw + S(7);
        }
        using (Font f = new Font("Segoe UI", 8.5f))
            TextRenderer.DrawText(g, "Program Files  \u00B7  administrator rights once (UAC)  \u00B7  version " + Engine.Version, f, new Point(S(40), S(H - 28 - 46 + 26)), Theme.Dim, TextFormatFlags.NoPadding);
        base.OnPaint(e);
    }

    // ------------------------------------------------------------ install pages
    private Panel WelcomePage()
    {
        Panel p = new ClearPanel();
        p.Controls.Add(Lbl("Downloads, fast and tidy.", 0, 0, CW, 30, 13.5f, Theme.Text, true));
        p.Controls.Add(Lbl("Pro Internet Manager downloads in segments, grabs videos from YouTube and HLS streams, and works with your browser through a local bridge.", 0, 34, CW, 46, 10f, Theme.Dim, false));
        Card card = CardAt(0, 92, CW, 190);
        card.Controls.Add(Lbl("What will be installed", 22, 14, 400, 26, 11f, Theme.Text, true));
        string[] items = { "Pro Internet Manager (themes Pro Lime, Midnight, Graphite and Azeroth)", "yt-dlp, ffmpeg and Deno (JavaScript runtime) for YouTube and merging video and audio", "Chromium extension (Chrome, Edge, Brave) as a folder, optional", "Link to the signed Firefox extension", "Uninstall.exe and an entry in Installed apps" };
        for (int i = 0; i < items.Length; i++)
        {
            card.Controls.Add(Lbl("\u25CF", 24, 50 + i * 26, 18, 24, 7f, Theme.Accent, false));
            card.Controls.Add(Lbl(items[i], 44, 48 + i * 26, 700, 24, 10f, Theme.Text, false));
        }
        p.Controls.Add(card);
        p.Controls.Add(Lbl("Installs to Program Files by default. Your settings are kept when you update.", 0, 296, CW, 40, 9f, Theme.Dim, false));
        return p;
    }

    private static string StdDir() { return Engine.StdDir(); }

    private Panel LocationPage()
    {
        Panel p = new ClearPanel();
        string std = StdDir();
        cardDefault = CardAt(0, 0, CW, 84);
        optDefault = new ToggleOption("defaultOption", "Default folder (recommended)", true) { Location = P(20, 10), Size = Sz(600, 32) };
        cardDefault.Controls.Add(optDefault);
        Label stdLbl = Lbl(std, 56, 48, 500, 24, 9.5f, Theme.Dim, false); stdLbl.AutoEllipsis = true;
        cardDefault.Controls.Add(stdLbl);
        freeDefault = Lbl(FreeText(std), 560, 48, 200, 24, 9.5f, Theme.Green, false);
        freeDefault.TextAlign = ContentAlignment.TopRight;
        cardDefault.Controls.Add(freeDefault);
        p.Controls.Add(cardDefault);

        cardCustom = CardAt(0, 98, CW, 140);
        optCustom = new ToggleOption("customOption", "Custom folder", true) { Location = P(20, 10), Size = Sz(500, 32) };
        cardCustom.Controls.Add(optCustom);
        freeCustom = Lbl("", 560, 16, 200, 24, 9.5f, Theme.Green, false);
        freeCustom.TextAlign = ContentAlignment.TopRight;
        cardCustom.Controls.Add(freeCustom);
        FieldBox field = new FieldBox("locationBox", false, new Font("Segoe UI", 10f)) { Location = P(56, 50), Size = Sz(CW - 56 - 20 - 170 - 12, 42) };
        TextBox box = field.Box;
        box.Text = defaultPath;
        cardCustom.Controls.Add(field);
        RoundButton browse = new RoundButton("Browse...", false) { Location = P(CW - 20 - 170, 50), Size = Sz(170, 42), Font = new Font("Segoe UI", 10f) };
        browse.Click += delegate
        {
            using (FolderBrowserDialog dialog = new FolderBrowserDialog())
            {
                dialog.Description = "Choose a folder for Pro Internet Manager";
                if (dialog.ShowDialog(this) == DialogResult.OK)
                {
                    string chosen = dialog.SelectedPath;
                    if (!chosen.TrimEnd('\\').EndsWith(Engine.AppName, StringComparison.OrdinalIgnoreCase)) chosen = Path.Combine(chosen, Engine.AppName);
                    box.Text = chosen;
                }
            }
        };
        cardCustom.Controls.Add(browse);
        cardCustom.Controls.Add(Lbl("Files from a previous version in this folder will be updated.", 56, 102, CW - 76, 24, 9f, Theme.Dim, false));
        p.Controls.Add(cardCustom);
        p.Controls.Add(Lbl("About 310 MB required (mostly ffmpeg and Deno). The default folder asks for administrator rights once.", 0, 252, CW, 26, 9f, Theme.Dim, false));

        location = box;
        optDefault.CheckedChanged += delegate
        {
            if (optDefault.Checked) { optCustom.Checked = false; settingPath = true; box.Text = std; settingPath = false; }
            UpdateLocationCards();
        };
        optCustom.CheckedChanged += delegate { if (optCustom.Checked) optDefault.Checked = false; UpdateLocationCards(); };
        box.TextChanged += delegate
        {
            if (!settingPath) optCustom.Checked = true;
            freeCustom.Text = optCustom.Checked ? FreeText(box.Text.Trim()) : "";
        };
        WireClick(cardDefault, delegate { optDefault.Checked = true; }, optDefault);
        WireClick(cardCustom, delegate { optCustom.Checked = true; }, optCustom);
        if (string.Equals(defaultPath.TrimEnd('\\'), std, StringComparison.OrdinalIgnoreCase)) optDefault.Checked = true;
        else optCustom.Checked = true;
        UpdateLocationCards();
        return p;
    }

    private void UpdateLocationCards()
    {
        cardDefault.Selected = optDefault.Checked;
        cardCustom.Selected = optCustom.Checked;
        location.ForeColor = optCustom.Checked ? Theme.Text : Theme.Dim;
        freeCustom.Text = optCustom.Checked ? FreeText(location.Text.Trim()) : "";
    }

    private static string FreeText(string path)
    {
        try
        {
            string root = Path.GetPathRoot(Path.GetFullPath(path));
            if (string.IsNullOrEmpty(root)) return "";
            DriveInfo d = new DriveInfo(root);
            if (d.IsReady) return (d.AvailableFreeSpace / 1073741824.0).ToString("0.0") + " GB free";
        }
        catch { }
        return "";
    }

    private ToggleOption OptionRow(Panel p, int y, string name, string text, string desc, bool on)
    {
        Card c = CardAt(0, y, CW, 62);
        ToggleOption t = new ToggleOption(name, text, false) { Location = P(20, 5), Size = Sz(CW - 40, 30), Checked = on };
        c.Controls.Add(t);
        Label d = Lbl(desc, 56, 35, CW - 76, 22, 9f, Theme.Dim, false); d.AutoEllipsis = true;
        c.Controls.Add(d);
        c.Selected = on;
        t.CheckedChanged += delegate { c.Selected = t.Checked; };
        WireClick(c, delegate { t.Checked = !t.Checked; }, t);
        p.Controls.Add(c);
        return t;
    }

    private Panel OptionsPage()
    {
        Panel p = new ClearPanel();
        optDesktop = OptionRow(p, 0, "desktopBox", "Desktop shortcut", "Pro Internet Manager.lnk on your desktop.", true);
        optStart = OptionRow(p, 70, "startBox", "Start menu shortcut", "Find the app via Start and search.", true);
        optAuto = OptionRow(p, 140, "autoBox", "Start with Windows (minimized)", "The bridge is then ready for the browser extension right away. Can be changed later in Options.", false);
        optChromium = OptionRow(p, 210, "chromiumBox", "Include Chromium extension (Chrome, Edge, Brave)", "Unpacked extension folder in the install folder. Firefox gets a link on the last screen.", true);
        optLaunch = OptionRow(p, 280, "launchBox", "Launch Pro Internet Manager after installation", "Starts the app when you click Finish.", true);
        return p;
    }

    private Panel ProgressPage()
    {
        Panel p = new ClearPanel();
        progressText = Lbl("Preparing...", 0, 0, CW, 28, 11.5f, Theme.Text, true);
        p.Controls.Add(progressText);
        progressPath = Lbl("", 0, 30, CW, 22, 9.5f, Theme.Dim, false);
        progressPath.AutoEllipsis = true;
        p.Controls.Add(progressPath);
        progress = new RoundProgress { Location = P(0, 66), Size = Sz(CW, 14) };
        p.Controls.Add(progress);
        progressPercent = Lbl("0%", 0, 92, 200, 34, 15f, Theme.Text, false);
        p.Controls.Add(progressPercent);
        progressEta = Lbl("", CW - 300, 98, 300, 26, 10f, Theme.Dim, false);
        progressEta.TextAlign = ContentAlignment.TopRight;
        p.Controls.Add(progressEta);
        FieldBox log = new FieldBox("logBox", true, new Font("Consolas", 8.5f)) { Location = P(0, 140), Size = Sz(CW, 200) };
        log.Box.ReadOnly = true;
        log.Box.ScrollBars = ScrollBars.Vertical;
        log.Box.ForeColor = Theme.Dim;
        logBox = log.Box;
        p.Controls.Add(log);
        return p;
    }

    private Panel FinishPage()
    {
        Panel p = new ClearPanel();
        if (uninstallMode)
        {
            p.Controls.Add(Lbl("Uninstall complete.", 0, 6, CW, 44, 18f, Theme.Text, true));
            p.Controls.Add(Lbl("You can close this window. Uninstall.exe and the empty folder are removed automatically once it is closed.", 0, 56, CW, 48, 10f, Theme.Dim, false));
            p.Controls.Add(Lbl("Remove browser extensions in the browser itself (about:addons or chrome://extensions).", 0, 112, CW, 40, 9f, Theme.Dim, false));
            return p;
        }
        p.Controls.Add(Lbl("Installation complete.", 0, 0, CW, 40, 18f, Theme.Text, true));
        p.Controls.Add(Lbl("Pro Internet Manager is ready. Connect your browser now, and links and videos go to the app with one click.", 0, 44, CW, 44, 10f, Theme.Dim, false));

        Card ff = CardAt(0, 96, CW, 110);
        ff.Controls.Add(Lbl("Firefox", 22, 12, 300, 26, 11.5f, Theme.Text, true));
        ff.Controls.Add(Lbl("Signed extension from addons.mozilla.org (Pro Internet Manager Bridge 0.3.2).", 22, 40, CW - 270, 24, 9.5f, Theme.Dim, false));
        ff.Controls.Add(Link(Engine.XpiUrl, 22, 70, CW - 270, 22, Engine.XpiUrl));
        RoundButton openFf = new RoundButton("Open in Firefox", true) { Location = P(CW - 20 - 210, 32), Size = Sz(210, 46) };
        openFf.Click += delegate { OpenFirefox(); };
        ff.Controls.Add(openFf);
        p.Controls.Add(ff);

        Card cr = CardAt(0, 218, CW, 110);
        cr.Controls.Add(Lbl("Chrome, Edge and Brave", 22, 12, 400, 26, 11.5f, Theme.Text, true));
        cr.Controls.Add(Lbl("Unpacked folder in the install folder, load it via Developer mode.", 22, 40, CW - 270, 24, 9.5f, Theme.Dim, false));
        cr.Controls.Add(Lbl("Instructions are also in chromium-extension\\INSTRUCTIONS.txt", 22, 70, CW - 270, 22, 9f, Theme.Dim, false));
        chromiumBtn = new RoundButton("Install Chromium extension", false) { Location = P(CW - 20 - 210, 32), Size = Sz(210, 46), Font = new Font("Segoe UI Semibold", 9.5f) };
        chromiumBtn.Click += delegate { ChromiumHelp.ShowFor(this, Path.Combine(InstallDir(), "chromium-extension"), true); };
        cr.Controls.Add(chromiumBtn);
        p.Controls.Add(cr);
        return p;
    }

    private void OpenFirefox()
    {
        string ff = Engine.Firefox();
        try
        {
            if (ff != null) Process.Start(new ProcessStartInfo(ff, "\"" + Engine.XpiUrl + "\"") { UseShellExecute = false });
            else
            {
                MessageBox.Show(this, "Firefox was not found. The link opens in your default browser; open it in Firefox to install the extension.", Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Information);
                Process.Start(new ProcessStartInfo(Engine.XpiUrl) { UseShellExecute = true });
            }
        }
        catch (Exception e) { MessageBox.Show(this, e.Message, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning); }
    }

    private Panel UninstallPage()
    {
        Panel p = new ClearPanel();
        p.Controls.Add(Lbl("Uninstall Pro Internet Manager", 0, 0, CW, 30, 13.5f, Theme.Text, true));
        Label where = Lbl("Folder: " + defaultPath, 0, 34, CW, 24, 10f, Theme.Dim, false); where.AutoEllipsis = true;
        p.Controls.Add(where);
        Card card = CardAt(0, 70, CW, 150);
        card.Controls.Add(Lbl("What will be removed", 22, 14, 400, 26, 11f, Theme.Text, true));
        string[] items = { "The files Setup placed in this folder", "Shortcuts on the desktop and in the Start menu", "'Start with Windows' and the entry in Installed apps" };
        for (int i = 0; i < items.Length; i++)
        {
            card.Controls.Add(Lbl("\u25CF", 24, 50 + i * 28, 18, 24, 7f, Theme.Accent, false));
            card.Controls.Add(Lbl(items[i], 44, 48 + i * 28, 700, 24, 10f, Theme.Text, false));
        }
        p.Controls.Add(card);
        Card c = CardAt(0, 234, CW, 84);
        optPurge = new ToggleOption("purgeBox", "Also remove settings and logs", false) { Location = P(20, 8), Size = Sz(CW - 40, 30) };
        c.Controls.Add(optPurge);
        c.Controls.Add(Lbl("%APPDATA%\\Pro Internet Downloader (settings, theme, logs) and downloaded updates. The folder of the original Rust Download Manager is not touched.", 56, 38, CW - 76, 42, 9f, Theme.Dim, false));
        optPurge.CheckedChanged += delegate { c.Selected = optPurge.Checked; };
        WireClick(c, delegate { optPurge.Checked = !optPurge.Checked; }, optPurge);
        p.Controls.Add(c);
        return p;
    }

    // ------------------------------------------------------------ navigation
    private string InstallDir() { return uninstallMode ? defaultPath : Path.GetFullPath(location.Text.Trim()).TrimEnd('\\'); }

    private void ShowPage(int index)
    {
        page = index;
        for (int i = 0; i < pages.Length; i++) pages[i].Visible = i == index;
        pageTitle.Text = steps[index];
        pageSub.Text = subs[index];
        int progressIndex = uninstallMode ? 1 : 3;
        int last = pages.Length - 1;
        if (index == progressIndex)
        {
            progressPath.Text = InstallDir();
            progressStart = DateTime.Now;
            progressEta.Text = "";
        }
        back.Visible = !uninstallMode && index > 0 && index < progressIndex;
        cancel.Visible = uninstallMode && index == 0;
        next.Enabled = index != progressIndex;
        if (uninstallMode) next.Text = index == 0 ? "Uninstall" : "Close";
        else next.Text = index == 2 ? "Install" : index == last ? "Finish" : "Next";
        Invalidate(true);
    }

    private void GoBack() { if (!busy && page > 0) ShowPage(page - 1); }

    private void GoNext()
    {
        if (busy) return;
        if (uninstallMode)
        {
            if (page == 0) StartWork();
            else if (page == 2) Close();
            return;
        }
        if (page == 0) ShowPage(1);
        else if (page == 1)
        {
            string path = location.Text.Trim();
            bool ok = path.Length >= 3 && path.IndexOfAny(Path.GetInvalidPathChars()) < 0;
            try { if (ok) Path.GetFullPath(path); } catch { ok = false; }
            if (!ok) { MessageBox.Show(this, "Choose a valid install folder.", Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning); return; }
            ShowPage(2);
        }
        else if (page == 2) StartWork();
        else if (page == 4)
        {
            if (optLaunch.Checked)
            {
                if (Engine.PortBusy(18500))
                    MessageBox.Show(this, "Port 18500 is already in use, probably by another download manager (for example the original Rust Download Manager). Pro Internet Manager will start, but the browser bridge only works once you close that other app.", Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Information);
                try { Engine.Launch(InstallDir()); } catch (Exception e) { MessageBox.Show(this, e.Message, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Warning); }
            }
            Close();
        }
    }

    // Ask (never kill) when the app runs from the target folder
    private bool WaitUntilClosed(string dir)
    {
        while (true)
        {
            List<string> running = Engine.RunningIn(dir);
            if (running.Count == 0) return true;
            DialogResult r = MessageBox.Show(this,
                "Pro Internet Manager is still running from this folder:\n\n  " + string.Join("\n  ", running.ToArray()) + "\n\nClose the app (including the system tray) and click Retry.",
                Engine.AppName, MessageBoxButtons.RetryCancel, MessageBoxIcon.Information);
            if (r != DialogResult.Retry) return false;
        }
    }

    private void StartWork()
    {
        string dir = InstallDir();
        if (!WaitUntilClosed(dir)) return;
        if (!EnsureWritable(dir)) return;
        dir = InstallDir();
        Log((uninstallMode ? "Uninstall from " : "Install to ") + dir);
        InstallOptions o = null;
        bool purge = false;
        if (uninstallMode) purge = optPurge.Checked;
        else o = new InstallOptions { Dir = dir, Desktop = optDesktop.Checked, StartMenu = optStart.Checked, Autostart = optAuto.Checked, Chromium = optChromium.Checked, Registry = true };
        busy = true;
        ShowPage(uninstallMode ? 1 : 3);
        Thread t = new Thread(delegate()
        {
            try
            {
                if (uninstallMode) Engine.Uninstall(dir, purge, Log, SetProgress);
                else Engine.Install(o, Log, SetProgress);
                BeginInvoke((MethodInvoker)delegate
                {
                    busy = false;
                    if (!uninstallMode) chromiumBtn.Enabled = o.Chromium;
                    ShowPage(uninstallMode ? 2 : 4);
                });
            }
            catch (Exception error)
            {
                Log("ERROR: " + error.Message.Replace("\n", " "));
                SetupLog.Write("ERROR details: " + error);
                BeginInvoke((MethodInvoker)delegate
                {
                    busy = false;
                    string msg = error is SetupWriteException ? error.Message : error.Message + "\n\nLog: " + SetupLog.FilePath;
                    MessageBox.Show(this, msg, Engine.AppName, MessageBoxButtons.OK, MessageBoxIcon.Error);
                    ShowPage(uninstallMode ? 0 : 2);
                });
            }
        });
        t.IsBackground = true;
        t.Start();
    }

    private void SetProgress(int percent, string message)
    {
        if (progress == null) return;
        if (InvokeRequired) { BeginInvoke((MethodInvoker)delegate { SetProgress(percent, message); }); return; }
        int v = Math.Max(0, Math.Min(100, percent));
        progress.Value = v;
        progressText.Text = message;
        progressPercent.Text = v + "%";
        if (v >= 100) progressEta.Text = "";
        else if (v > 0 && progressStart != DateTime.MinValue)
        {
            double elapsed = (DateTime.Now - progressStart).TotalSeconds;
            if (elapsed > 2)
            {
                TimeSpan left = TimeSpan.FromSeconds(elapsed * (100 - v) / v);
                progressEta.Text = "About " + (int)left.TotalMinutes + ":" + left.Seconds.ToString("00") + " left";
            }
        }
    }

    private void Log(string line)
    {
        SetupLog.Write(line);
        string text = DateTime.Now.ToString("HH:mm:ss") + "  " + line;
        lock (logLock) logPending.Enqueue(text);
    }

    private void FlushLog()
    {
        if (logBox == null || logBox.IsDisposed) return;
        string[] batch;
        lock (logLock)
        {
            if (logPending.Count == 0) return;
            batch = logPending.ToArray();
            logPending.Clear();
        }
        int start = Math.Max(0, batch.Length - LogTrimTo);
        int added = batch.Length - start;
        StringBuilder sb = new StringBuilder();
        if (logLines + added > LogMaxLines)
        {
            int keepOld = LogTrimTo - added;
            if (keepOld > 0)
            {
                string[] old = logBox.Text.Split(new[] { Environment.NewLine }, StringSplitOptions.RemoveEmptyEntries);
                for (int i = Math.Max(0, old.Length - keepOld); i < old.Length; i++) sb.Append(old[i]).Append(Environment.NewLine);
                logLines = Math.Min(old.Length, keepOld);
            }
            else logLines = 0;
            for (int i = start; i < batch.Length; i++) sb.Append(batch[i]).Append(Environment.NewLine);
            logBox.Text = sb.ToString();
        }
        else
        {
            for (int i = start; i < batch.Length; i++) sb.Append(batch[i]).Append(Environment.NewLine);
            logBox.AppendText(sb.ToString());
        }
        logLines += added;
        logBox.SelectionStart = logBox.TextLength;
        logBox.SelectionLength = 0;
        logBox.ScrollToCaret();
    }

#if PREVIEW
    internal void PreviewShow(int index)
    {
        int progressIndex = uninstallMode ? 1 : 3;
        ShowPage(index);
        if (index == progressIndex)
        {
            progressStart = DateTime.Now.AddSeconds(-9);
            SetProgress(46, "Extracting: ffmpeg.exe");
            Log("Preview - nothing is copied");
            Log("Installing to " + InstallDir());
            Log("Extracting: 53 files, 213.6 MB");
            Log("Placed Pro Internet Manager.exe  (23.3 MB)");
            Log("Placed ProInternetManager.ico  (35 KB)");
            string[] th = { "lime", "midnight", "graphite", "azeroth" };
            string[] sz = { "16", "32", "48", "96", "128" };
            for (int n = 0; n < 20; n++) Log("Placed chromium-extension\\icons\\" + th[n % 4] + "\\" + sz[n % 5] + ".png  (2 KB)");
            Log("Placed FFMPEG-NOTICE.txt  (483 B)");
        }
        FlushLog();
    }
#endif
}

// ---------------------------------------------------------------- Chromium instruction page
internal sealed class ChromiumHelp : ThemedForm
{
    public static void ShowFor(IWin32Window owner, string folder, bool openFolder)
    {
        using (ChromiumHelp f = new ChromiumHelp(folder, openFolder)) f.ShowDialog(owner);
    }

    private static void Explore(string folder)
    {
        try { if (Directory.Exists(folder)) Process.Start(new ProcessStartInfo("explorer.exe", "\"" + folder + "\"") { UseShellExecute = true }); } catch { }
    }

    public ChromiumHelp(string folder, bool openFolder)
    {
        CW = 640;
        HeaderTitle = "Chromium extension";
        Text = "Install Chromium extension";
        ShowInTaskbar = false;
        ClientSize = Sz(720, 610);
        if (openFolder) Explore(folder);
        Controls.Add(Lbl("Chrome, Edge and Brave: load the folder as an unpacked extension", 40, 146, CW, 30, 12.5f, Theme.Text, true));
        string[] stepsText =
        {
            "Open in your browser:  chrome://extensions   \u00B7   edge://extensions   \u00B7   brave://extensions",
            "Turn on 'Developer mode'. Chrome and Brave: top right. Edge: on the left.",
            "Click 'Load unpacked'.",
            "Choose this folder (already open in File Explorer):"
        };
        for (int i = 0; i < stepsText.Length; i++)
        {
            Label n = Lbl((i + 1).ToString(), 40, 192 + i * 40, 28, 28, 11f, Theme.AccentText, true);
            n.BackColor = Theme.Accent; n.TextAlign = ContentAlignment.MiddleCenter;
            Controls.Add(n);
            Controls.Add(Lbl(stepsText[i], 80, 195 + i * 40, CW - 40, 28, 10f, Theme.Text, false));
        }
        FieldBox field = new FieldBox("folderBox", false, new Font("Segoe UI", 9.5f)) { Location = P(80, 350), Size = Sz(CW - 40 - 150 - 10, 40) };
        field.Box.Text = folder; field.Box.ReadOnly = true;
        Controls.Add(field);
        RoundButton copy = new RoundButton("Copy path", false) { Location = P(80 + CW - 40 - 150, 350), Size = Sz(150, 40), Font = new Font("Segoe UI", 10f) };
        copy.Click += delegate { try { Clipboard.SetText(folder); copy.Text = "Copied"; } catch { } };
        Controls.Add(copy);
        Controls.Add(Lbl("Chrome may show a warning about developer mode extensions at startup. That is normal for an unpacked extension; just dismiss it. Keep this folder: if you move or delete it, the extension stops working.", 40, 404, CW, 62, 9.5f, Theme.Dim, false));

        int x = 40;
        AddBrowser(ref x, "Chrome", Engine.Chrome(), "chrome://extensions");
        AddBrowser(ref x, "Edge", Engine.Edge(), "edge://extensions");
        AddBrowser(ref x, "Brave", Engine.Brave(), "brave://extensions");
        RoundButton open = new RoundButton("Open folder", false) { Location = P(x, 480), Size = Sz(130, 40), Font = new Font("Segoe UI", 10f) };
        open.Click += delegate { Explore(folder); };
        Controls.Add(open);
        RoundButton close = new RoundButton("Close", true) { Location = P(720 - 40 - 150, 610 - 28 - 46), Size = Sz(150, 46) };
        close.Click += delegate { Close(); };
        Controls.Add(close);
        AcceptButton = close;
        ActiveControl = close;
        Shown += delegate { field.Box.SelectionStart = 0; field.Box.SelectionLength = 0; };
    }

    private void AddBrowser(ref int x, string name, string exe, string page)
    {
        if (exe == null) return;
        RoundButton b = new RoundButton("Open " + name, false) { Location = P(x, 480), Size = Sz(130, 40), Font = new Font("Segoe UI", 10f) };
        b.Click += delegate { try { Process.Start(new ProcessStartInfo(exe, page) { UseShellExecute = false }); } catch { } };
        Controls.Add(b);
        x += 140;
    }
}

#if PREVIEW
internal static class Preview
{
    [DllImport("user32.dll")]
    private static extern bool PrintWindow(IntPtr hwnd, IntPtr hdc, uint flags);

    static void Shot(Form f, Action prepare, string file)
    {
        f.StartPosition = FormStartPosition.Manual;
        f.Location = new Point(20, 20);
        f.ShowInTaskbar = false;
        f.Opacity = 0.01;
        f.Show();
        if (prepare != null) prepare();
        f.ActiveControl = null;
        for (int k = 0; k < 20; k++) { Application.DoEvents(); Thread.Sleep(25); }
        f.Opacity = 1;
        for (int k = 0; k < 10; k++) { Application.DoEvents(); Thread.Sleep(25); }
        using (Bitmap bmp = new Bitmap(f.Width, f.Height))
        {
            using (Graphics g = Graphics.FromImage(bmp))
            {
                IntPtr hdc = g.GetHdc();
                bool ok = PrintWindow(f.Handle, hdc, 2);
                g.ReleaseHdc(hdc);
                if (!ok) f.DrawToBitmap(bmp, new Rectangle(0, 0, f.Width, f.Height));
            }
            bmp.Save(file, System.Drawing.Imaging.ImageFormat.Png);
        }
        f.Close();
        f.Dispose();
    }

    public static void Run(string outDir)
    {
        Directory.CreateDirectory(outDir);
        string std = Engine.StdDir();
        string[] names = { "1_welcome", "2_location", "3_options", "4_installing", "5_finished" };
        for (int i = 0; i < names.Length; i++)
        {
            int idx = i;
            SetupForm f = new SetupForm(false, std);
            Shot(f, delegate { f.PreviewShow(idx); }, Path.Combine(outDir, names[i] + ".png"));
        }
        string[] un = { "6_uninstall", "7_uninstall_working" };
        for (int i = 0; i < un.Length; i++)
        {
            int idx = i;
            SetupForm f = new SetupForm(true, std);
            Shot(f, delegate { f.PreviewShow(idx); }, Path.Combine(outDir, un[i] + ".png"));
        }
        Shot(new ChromiumHelp(Path.Combine(std, "chromium-extension"), false), null, Path.Combine(outDir, "8_chromium_help.png"));
    }
}
#endif
