using System.Collections;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;

// Research only. No account, policy, privilege or service mutations here.
internal static class Program
{
    static bool payload;
    static int lines;
    static int Main(string[] args)
    {
        try
        {
            if (args.SequenceEqual(new[] { "--self-test" })) return SelfTest();
            if (args.SequenceEqual(new[] { "--handle-fixture" }))
            {
                // Fixed unprivileged diagnostic; no environment, file, token or account changes.
                int mask = 0;
                for (int i = 0; i < 3; i++) { nint h = Native.GetStdHandle(-10 - i); if (h != 0 && h != -1) mask |= 1 << i; }
                return mask;
            }
            GuardCi(Environment.GetEnvironmentVariable);
            if (args.SequenceEqual(new[] { "--child" })) { payload = true; return Child(false); }
            if (args.SequenceEqual(new[] { "--leaf" })) { payload = true; return Child(true); }
            Require(args.Length == 2, "arguments: account workspace required");
            ValidateAccount(args[0]);
            string workspace = ValidateWorkspace(args[1], Environment.GetEnvironmentVariable);
            using var identity = WindowsIdentity.GetCurrent();
            Require(identity.User?.Value == "S-1-5-18", "preflight: LocalSystem required; no elevation or fallback");
            foreach (string privilege in new[] { "SeTcbPrivilege", "SeAssignPrimaryTokenPrivilege", "SeIncreaseQuotaPrivilege" })
                Native.RequirePrivilege(identity.Token, privilege, privilege == "SeTcbPrivilege");
            Require(!Directory.EnumerateFileSystemEntries(workspace).Any(), "preflight: workspace must be empty");
            string sid = Native.LocalStandardAccount(args[0]);
            Emit(new { stage = "preflight", ok = true, expectedSid = sid });
            using var token = Native.S4u(args[0]);
            Native.Observe(token.Handle, sid, "logon-token");
            Run(token.Handle, workspace, sid);
            return 0;
        }
        catch (ProbeFailure ex)
        {
            if (lines < 128) Emit(new { stage = "failure", ok = false, reason = ex.Message });
            return payload && ex.NativeError != 0 ? unchecked((int)(0xe0000000u | ex.NativeError)) : 1;
        }
        // Never print exception messages, paths, environment values or stack traces.
        catch (Exception) { if (lines < 128) Emit(new { stage = "failure", ok = false, reason = "unexpected-managed-failure" }); return 2; }
    }

    internal static void Require(bool value, string reason) { if (!value) throw new ProbeFailure(reason); }
    internal static void Emit(object value)
    {
        if (payload) return; // Payload has no inherited stdout/stderr and communicates by exit status.
        string json = JsonSerializer.Serialize(value);
        Require(json.Length <= 2048 && ++lines <= 128, "output-bound");
        Console.WriteLine(json);
    }
    internal static void ValidateAccount(string account) => Require(Regex.IsMatch(account, "\\Aaxp[0-9a-f]{8}\\z"), "account: generated axp + eight hex digits required");
    internal static void GuardCi(Func<string, string?> env)
    {
        Require(OperatingSystem.IsWindows(), "preflight: Windows required");
        Require(env("GITHUB_ACTIONS") == "true" && env("RUNNER_ENVIRONMENT") == "github-hosted" &&
            env("RUNNER_OS") == "Windows" && env("ARXCOVO_DISPOSABLE_PROBE") == "reviewed-manual-ci-v1" &&
            Regex.IsMatch(env("GITHUB_RUN_ID") ?? "", "\\A[0-9]{1,20}\\z") &&
            Regex.IsMatch(env("GITHUB_RUN_ATTEMPT") ?? "", "\\A[0-9]{1,5}\\z"), "preflight: reviewed disposable hosted Windows CI required");
    }
    internal static string ValidateWorkspace(string path, Func<string, string?> env)
    {
        string expected = @"C:\arxcovo-probe\" + env("GITHUB_RUN_ID") + "-" + env("GITHUB_RUN_ATTEMPT") + @"\work";
        Require(string.Equals(path, expected, StringComparison.OrdinalIgnoreCase), "workspace: fixed disposable location required");
        CheckPath(path);
        return path;
    }
    internal static void CheckPath(string path)
    {
        // No canonicalization through links; reject every existing component including drive root.
        for (DirectoryInfo? part = new DirectoryInfo(path); part != null; part = part.Parent)
            Require(part.Exists && (part.Attributes & FileAttributes.ReparsePoint) == 0, "path: missing directory or reparse point");
    }
    internal static string EnvironmentBlock(string run, string attempt, string windows) =>
        "ARXCOVO_DISPOSABLE_PROBE=reviewed-manual-ci-v1\0GITHUB_ACTIONS=true\0GITHUB_RUN_ATTEMPT=" + attempt +
        "\0GITHUB_RUN_ID=" + run + "\0RUNNER_ENVIRONMENT=github-hosted\0RUNNER_OS=Windows\0SystemRoot=" + windows + "\0\0";

    static int Child(bool leaf)
    {
        using var identity = WindowsIdentity.GetCurrent();
        Require(identity.User?.Value != "S-1-5-18", "child: refusing SYSTEM");
        var allowed = new HashSet<string>(new[] { "ARXCOVO_DISPOSABLE_PROBE", "GITHUB_ACTIONS", "GITHUB_RUN_ATTEMPT", "GITHUB_RUN_ID", "RUNNER_ENVIRONMENT", "RUNNER_OS", "SystemRoot" }, StringComparer.OrdinalIgnoreCase);
        foreach (DictionaryEntry entry in Environment.GetEnvironmentVariables())
            Require(allowed.Contains((string)entry.Key), "child: unexpected environment key");
        foreach (int kind in new[] { -10, -11, -12 })
        {
            nint handle = Native.GetStdHandle(kind);
            Require(handle == 0 || handle == -1, "child: unexpected standard handle");
        }
        if (leaf) { Thread.Sleep(30000); return 44; }
        Native.ProcessInfo pi = Native.Create(0, "--leaf", Environment.CurrentDirectory);
        using var process = new OwnedHandle(pi.process); using var thread = new OwnedHandle(pi.thread);
        Native.Win(Native.ResumeThread(thread.Handle) != uint.MaxValue, "ResumeThread-leaf");
        return 42; // Managed startup reached; environment/standard handles checked; descendant created.
    }
    static void Run(nint token, string workspace, string sid)
    {
        using var job = new OwnedHandle(Native.CreateJobObjectW(0, null));
        Native.Win(job.Handle != 0, "CreateJobObjectW");
        var limits = new Native.JobLimits { basic = new Native.BasicLimits { flags = 0x2000 | 8, activeLimit = 3 } };
        Native.Win(Native.SetInformationJobObject(job.Handle, 9, ref limits, Marshal.SizeOf<Native.JobLimits>()), "SetInformationJobObject");
        Native.ProcessInfo pi = Native.Create(token, "--child", workspace);
        using var process = new OwnedHandle(pi.process); using var thread = new OwnedHandle(pi.thread);
        bool assigned = false;
        try
        {
            // No code executes before assignment; failure always kills suspended root.
            Native.Win(Native.AssignProcessToJobObject(job.Handle, process.Handle), "AssignProcessToJobObject");
            assigned = true;
            Native.ProcessIdentity(process.Handle, sid, "suspended-child");
            Native.Win(Native.ResumeThread(thread.Handle) != uint.MaxValue, "ResumeThread");
            uint wait = Native.WaitForSingleObject(process.Handle, 10000);
            Native.Win(wait != uint.MaxValue, "WaitForSingleObject-child");
            Emit(new { stage = "child-wait", result = wait });
            Require(wait == 0, "child: startup timeout or wait failure");
            Native.Win(Native.GetExitCodeProcess(process.Handle, out uint code), "GetExitCodeProcess");
            Emit(new { stage = "child-exit", exitCode = code, payloadNativeError = (code & 0xf0000000) == 0xe0000000 ? (uint?)(code & 0x0fffffff) : null });
            Require(code == 42, "child: managed startup/environment/handles/descendant failed");
            uint[] pids = Native.JobPids(job.Handle);
            Require(pids.Length == 1, "job: exactly one live descendant expected");
            using var descendant = new OwnedHandle(Native.OpenProcess(0x1000 | 0x100000, false, pids[0]));
            Native.Win(descendant.Handle != 0, "OpenProcess-descendant");
            Native.Win(Native.IsProcessInJob(descendant.Handle, job.Handle, out bool inJob), "IsProcessInJob");
            Require(inJob, "descendant: job membership missing");
            Native.ProcessIdentity(descendant.Handle, sid, "descendant");
            Native.Win(Native.TerminateJobObject(job.Handle, 124), "TerminateJobObject");
            Require(Native.WaitForSingleObject(descendant.Handle, 5000) == 0, "descendant: termination timeout");
            var watch = Stopwatch.StartNew();
            while (Native.JobPids(job.Handle).Length != 0 && watch.ElapsedMilliseconds < 5000) Thread.Sleep(20);
            Require(Native.JobPids(job.Handle).Length == 0, "job: live processes after termination");
            Native.Win(Native.GetExitCodeProcess(descendant.Handle, out uint descendantExit), "GetExitCodeProcess-descendant");
            Require(descendantExit == 124, "descendant: unexpected termination code");
            Emit(new { stage = "bounded-tree-probe", ok = true, descendantExitCode = descendantExit, liveJobProcesses = 0, productionAcceptance = false });
        }
        finally
        {
            if (assigned) Native.Win(Native.TerminateJobObject(job.Handle, 124), "cleanup-TerminateJobObject");
            else Native.Win(Native.TerminateProcess(process.Handle, 124), "cleanup-TerminateProcess");
            Require(Native.WaitForSingleObject(process.Handle, 5000) == 0, "cleanup: root termination timeout");
            if (assigned)
            {
                var cleanupWatch = Stopwatch.StartNew();
                while (Native.JobPids(job.Handle).Length != 0 && cleanupWatch.ElapsedMilliseconds < 5000) Thread.Sleep(20);
                Require(Native.JobPids(job.Handle).Length == 0, "cleanup: job drain timeout");
            }
        }
    }

    static int SelfTest()
    {
        int count = 0;
        void Reject(Action test) { try { test(); } catch (ProbeFailure) { count++; return; } throw new ProbeFailure("self-test: expected refusal"); }
        ValidateAccount("axp0123abcd"); count++;
        foreach (string bad in new[] { "Administrator", "axp0123abcD", "axp0123abcd\\x", "axp0123abcd\n", "axp0123abcd extra", "" }) Reject(() => ValidateAccount(bad));
        var env = new Dictionary<string, string> { ["GITHUB_ACTIONS"] = "true", ["RUNNER_ENVIRONMENT"] = "github-hosted", ["RUNNER_OS"] = "Windows", ["ARXCOVO_DISPOSABLE_PROBE"] = "reviewed-manual-ci-v1", ["GITHUB_RUN_ID"] = "123", ["GITHUB_RUN_ATTEMPT"] = "1" };
        string? Read(string name) => env.GetValueOrDefault(name);
        GuardCi(Read); count++;
        foreach (string key in env.Keys.ToArray()) { string old = env[key]; env[key] = ""; Reject(() => GuardCi(Read)); env[key] = old; }
        Reject(() => ValidateWorkspace(@"C:\Windows", Read));
        Reject(() => ValidateWorkspace(@"C:\arxcovo-probe\123-1\work\..\work", Read));
        Reject(() => CheckPath(Path.Combine(Path.GetTempPath(), Guid.NewGuid().ToString("N"))));
        Require(JsonSerializer.Serialize(new { stage = "test", code = 1314 }).Contains("1314"), "self-test: serialization"); count++;
        var block = EnvironmentBlock("123", "1", @"C:\Windows");
        Require(block.EndsWith("\0\0", StringComparison.Ordinal) && block.Split('\0', StringSplitOptions.RemoveEmptyEntries).Length == 7 && !block.Contains("PATH="), "self-test: environment allowlist"); count++;
        Require(Marshal.SizeOf<Native.UnicodeString>() == (IntPtr.Size == 8 ? 16 : 8), "self-test: UNICODE_STRING layout"); count++;
        Require(Marshal.SizeOf<Native.JobLimits>() == (IntPtr.Size == 8 ? 144 : 112), "self-test: job layout"); count++;
        Require(Native.HandleFixture(0x08000404) == 7, "self-test: hidden-console regression control"); count++;
        Require(Native.HandleFixture(Native.CreationFlags) == 0, "self-test: detached handles"); count++;
        Native.RejectPrivilegedGroups(new[] { ("S-1-5-32-545", 4u) }); count++;
        foreach (string group in new[] { "S-1-5-32-544", "S-1-5-114", "S-1-5-21-1-2-3-512" })
            foreach (uint attributes in new[] { 4u, 16u }) Reject(() => Native.RejectPrivilegedGroups(new[] { (group, attributes) }));
        using var current = WindowsIdentity.GetCurrent();
        var rawGroups = Native.ReadGroups(current.Token);
        var denyAdmin = rawGroups.Where(g => g.Sid == "S-1-5-32-544" && (g.Attributes & 16) != 0).ToArray();
        // UAC limited tokens have a linked full token. Check that independently
        // so filtering deny-only groups cannot silently skip this regression.
        if (Native.ElevationType(current.Token) == 3)
        {
            Require(denyAdmin.Length == 1, "self-test: limited token lost deny-only Administrators"); count++;
        }
        // Existing filtered admin token only; never create accounts, change UAC or elevate.
        if (denyAdmin.Length != 0)
        {
            Require(!(current.Groups?.Any(g => g.Value == "S-1-5-32-544") ?? false), "self-test: filtered admin control"); count++;
            Reject(() => Native.RejectPrivilegedGroups(rawGroups));
        }
        using var emptyJob = new OwnedHandle(Native.CreateJobObjectW(0, null));
        Require(emptyJob.Handle != 0, "self-test: empty job creation");
        int linesBeforePolling = lines;
        for (int i = 0; i < 300; i++) Require(Native.JobPids(emptyJob.Handle).Length == 0, "self-test: empty job query");
        Require(lines == linesBeforePolling, "self-test: drain poll output bound"); count++;
        Emit(new { stage = "raw-groups-self-test", denyOnlyAdminObserved = denyAdmin.Length != 0 });
        Emit(new { stage = "self-test", ok = true, assertions = count, privilegedPathsRun = false });
        return 0;
    }
}
internal sealed class ProbeFailure(string message, uint nativeError = 0) : Exception(message)
{
    internal uint NativeError { get; } = nativeError;
}
internal sealed class OwnedHandle(nint value) : IDisposable
{
    public nint Handle { get; private set; } = value;
    public void Dispose() { if (Handle != 0) { Native.CloseHandle(Handle); Handle = 0; } }
}
