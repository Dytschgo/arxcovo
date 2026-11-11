using System.Runtime.InteropServices;
using System.Security.Principal;
using System.Text;

internal static class Native
{
    internal static void Win(bool ok, string api)
    {
        int error = ok ? 0 : Marshal.GetLastWin32Error();
        Program.Emit(new { stage = "win32", api, ok, error });
        if (!ok) throw new ProbeFailure(api + " failed", unchecked((uint)error));
    }
    static void Nt(uint status, string api, uint subStatus = 0)
    {
        Program.Emit(new { stage = "lsa", api, status, subStatus, win32 = LsaNtStatusToWinError(status), subWin32 = LsaNtStatusToWinError(subStatus) });
        Program.Require(status == 0, api + " failed");
    }
    internal static void RequirePrivilege(nint token, string name, bool mustBeEnabled)
    {
        Win(LookupPrivilegeValueW(null, name, out var luid), "LookupPrivilegeValueW");
        var set = new PrivilegeSet { count = 1, control = 1, luid = luid, attributes = 2 };
        Win(PrivilegeCheck(token, ref set, out bool enabled), "PrivilegeCheck");
        nint data = TokenInfo(token, 3);
        bool present = false;
        try
        {
            int count = Marshal.ReadInt32(data);
            Program.Require(count is >= 0 and <= 128, "preflight: privilege list invalid");
            for (int i = 0; i < count; i++)
            {
                var candidate = Marshal.PtrToStructure<Luid>(data + 4 + i * 12);
                present |= candidate.low == luid.low && candidate.high == luid.high;
            }
        }
        finally { Marshal.FreeHGlobal(data); }
        Program.Emit(new { stage = "privilege", name, present, enabled, mustBeEnabled });
        Program.Require(present && (!mustBeEnabled || enabled), "preflight: required privilege absent or ineffective; no fallback");
    }
    internal static string LocalStandardAccount(string name)
    {
        uint status = NetUserGetInfo(null, name, 1, out nint data);
        Program.Emit(new { stage = "account", api = "NetUserGetInfo", status });
        Program.Require(status == 0, "account: local generated user absent or inaccessible");
        try
        {
            var user = Marshal.PtrToStructure<UserInfo1>(data);
            Program.Require(user.privilege == 1 && (user.flags & 2) == 0 && (user.flags & 0x200) != 0, "account: enabled normal standard user required");
        }
        finally { NetApiBufferFree(data); }
        status = NetUserGetLocalGroups(null, name, 0, 1, out data, 16384, out uint read, out uint total);
        Program.Emit(new { stage = "account", api = "NetUserGetLocalGroups", status });
        try
        {
            Program.Require(status == 0 && read == total && read <= 16, "account: group enumeration incomplete");
            for (int i = 0; i < read; i++)
            {
                string group = Marshal.PtrToStringUni(Marshal.ReadIntPtr(data, i * IntPtr.Size))!;
                string groupSid = ((SecurityIdentifier)new NTAccount(Environment.MachineName, group).Translate(typeof(SecurityIdentifier))).Value;
                Program.Require(groupSid == "S-1-5-32-545", "account: membership outside BUILTIN Users rejected");
            }
        }
        finally { if (data != 0) NetApiBufferFree(data); }
        return ((SecurityIdentifier)new NTAccount(Environment.MachineName, name).Translate(typeof(SecurityIdentifier))).Value;
    }
    internal static OwnedHandle S4u(string name)
    {
        using var origin = new Ansi("ArxProbe");
        Nt(LsaRegisterLogonProcess(ref origin.Value, out nint lsa, out _), "LsaRegisterLogonProcess");
        try
        {
            using var package = new Ansi("MICROSOFT_AUTHENTICATION_PACKAGE_V1_0");
            Nt(LsaLookupAuthenticationPackage(lsa, ref package.Value, out uint id), "LsaLookupAuthenticationPackage");
            string domain = Environment.MachineName;
            int size = Marshal.SizeOf<S4uLogon>();
            int userBytes = Encoding.Unicode.GetByteCount(name), domainBytes = Encoding.Unicode.GetByteCount(domain);
            int length = size + userBytes + 2 + domainBytes + 2;
            nint buffer = Marshal.AllocHGlobal(length);
            try
            {
                byte[] user = Encoding.Unicode.GetBytes(name + "\0"), realm = Encoding.Unicode.GetBytes(domain + "\0");
                Marshal.Copy(user, 0, buffer + size, user.Length);
                Marshal.Copy(realm, 0, buffer + size + user.Length, realm.Length);
                var logon = new S4uLogon { type = 12, flags = 2, user = new UnicodeString { length = (ushort)userBytes, maximum = (ushort)user.Length, buffer = buffer + size }, domain = new UnicodeString { length = (ushort)domainBytes, maximum = (ushort)realm.Length, buffer = buffer + size + user.Length } };
                Marshal.StructureToPtr(logon, buffer, false);
                Win(AllocateLocallyUniqueId(out var luid), "AllocateLocallyUniqueId");
                var source = new TokenSource { name = Encoding.ASCII.GetBytes("ArxProbe"), id = luid };
                uint status = LsaLogonUser(lsa, ref origin.Value, 4, id, buffer, (uint)length, 0, ref source,
                    out nint profile, out _, out _, out nint token, out _, out uint sub);
                if (profile != 0) LsaFreeReturnBuffer(profile);
                var owned = new OwnedHandle(token);
                try { Nt(status, "LsaLogonUser-MSV1_0-S4U-Batch", sub); return owned; }
                catch { owned.Dispose(); throw; }
            }
            finally { Marshal.FreeHGlobal(buffer); }
        }
        finally { LsaDeregisterLogonProcess(lsa); }
    }
    static nint TokenInfo(nint token, int kind)
    {
        GetTokenInformation(token, kind, 0, 0, out int length);
        Program.Require(length is > 0 and <= 65536, "token: information length invalid");
        nint data = Marshal.AllocHGlobal(length);
        try { Win(GetTokenInformation(token, kind, data, length, out _), "GetTokenInformation"); return data; }
        catch { Marshal.FreeHGlobal(data); throw; }
    }
    internal static void Observe(nint token, string expected, string stage)
    {
        using var identity = new WindowsIdentity(token);
        nint data = TokenInfo(token, 25);
        uint integrity;
        try
        {
            string value = new SecurityIdentifier(Marshal.ReadIntPtr(data)).Value;
            integrity = uint.Parse(value.Split('-')[^1], System.Globalization.CultureInfo.InvariantCulture);
        }
        finally { Marshal.FreeHGlobal(data); }
        data = TokenInfo(token, 8);
        int tokenType;
        try { tokenType = Marshal.ReadInt32(data); }
        finally { Marshal.FreeHGlobal(data); }
        string? sid = identity.User?.Value;
        Program.Emit(new { stage, observedSid = sid, integrityRid = integrity, mediumIntegrity = integrity == 8192, tokenType });
        Program.Require(sid == expected && integrity == 8192 && tokenType == 1, "token: SID/Medium integrity/primary type mismatch");
        // Reject privileged memberships even if deny-only (filtered admin is not a standard account).
        foreach (IdentityReference group in identity.Groups ?? new IdentityReferenceCollection())
        {
            string value = group.Value;
            Program.Require(!value.StartsWith("S-1-5-32-", StringComparison.Ordinal) || value == "S-1-5-32-545", "token: privileged or unexpected builtin membership");
            Program.Require(!RegexPrivilegedDomainGroup(value), "token: privileged domain membership");
        }
        data = TokenInfo(token, 3);
        try
        {
            int count = Marshal.ReadInt32(data);
            Program.Require(count is >= 0 and <= 128, "token: privilege count invalid");
            for (int i = 0; i < count; i++)
            {
                var luid = Marshal.PtrToStructure<Luid>(data + 4 + i * 12);
                var name = new StringBuilder(128); int length = name.Capacity;
                Win(LookupPrivilegeNameW(null, ref luid, name, ref length), "LookupPrivilegeNameW");
                Program.Require(new[] { "SeChangeNotifyPrivilege", "SeIncreaseWorkingSetPrivilege", "SeTimeZonePrivilege", "SeShutdownPrivilege", "SeUndockPrivilege" }.Contains(name.ToString()), "token: unexpected privilege (even disabled)");
            }
        }
        finally { Marshal.FreeHGlobal(data); }
    }
    static bool RegexPrivilegedDomainGroup(string sid) => sid.StartsWith("S-1-5-21-", StringComparison.Ordinal) && new[] { "-512", "-516", "-518", "-519", "-520" }.Any(sid.EndsWith);
    internal static void ProcessIdentity(nint process, string sid, string stage)
    {
        Win(OpenProcessToken(process, 8, out nint token), "OpenProcessToken");
        using var owned = new OwnedHandle(token);
        Observe(token, sid, stage);
    }
    internal static ProcessInfo Create(nint token, string mode, string directory)
    {
        // Must publish self-contained: no dotnet host arguments or PATH search.
        string exe = Environment.ProcessPath!;
        Program.Require(Path.GetFileName(exe).Equals("IdentityProbe.exe", StringComparison.OrdinalIgnoreCase), "publish: run published IdentityProbe.exe");
        Program.CheckPath(Path.GetDirectoryName(exe)!);
        Program.Require((File.GetAttributes(exe) & FileAttributes.ReparsePoint) == 0, "executable: reparse point");
        var command = new StringBuilder("\"" + exe + "\" " + mode);
        nint environment = Marshal.StringToHGlobalUni(Program.EnvironmentBlock(Environment.GetEnvironmentVariable("GITHUB_RUN_ID")!, Environment.GetEnvironmentVariable("GITHUB_RUN_ATTEMPT")!, Environment.GetFolderPath(Environment.SpecialFolder.Windows)));
        try
        {
            var startup = new StartupInfo { size = Marshal.SizeOf<StartupInfo>() };
            // CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | CREATE_NO_WINDOW; no desktop override.
            bool ok = token == 0 ? CreateProcessW(exe, command, 0, 0, false, 0x08000404, environment, directory, ref startup, out var pi)
                : CreateProcessAsUserW(token, exe, command, 0, 0, false, 0x08000404, environment, directory, ref startup, out pi);
            Win(ok, token == 0 ? "CreateProcessW-leaf" : "CreateProcessAsUserW-suspended");
            return pi;
        }
        finally { Marshal.FreeHGlobal(environment); }
    }
    internal static uint[] JobPids(nint job)
    {
        // Active process limit is three. Bounded buffer, fail rather than truncate.
        nint buffer = Marshal.AllocHGlobal(8 + 16 * IntPtr.Size);
        try
        {
            Win(QueryInformationJobObject(job, 3, buffer, 8 + 16 * IntPtr.Size, out _), "QueryInformationJobObject");
            int assigned = Marshal.ReadInt32(buffer), count = Marshal.ReadInt32(buffer, 4);
            Program.Require(count >= 0 && count <= 3 && count == assigned, "job: PID list incomplete");
            return Enumerable.Range(0, count).Select(i => checked((uint)Marshal.ReadIntPtr(buffer, 8 + i * IntPtr.Size))).ToArray();
        }
        finally { Marshal.FreeHGlobal(buffer); }
    }
    sealed class Ansi : IDisposable
    {
        public UnicodeString Value;
        public Ansi(string value) { Value.buffer = Marshal.StringToHGlobalAnsi(value); Value.length = (ushort)value.Length; Value.maximum = (ushort)(value.Length + 1); }
        public void Dispose() => Marshal.FreeHGlobal(Value.buffer);
    }
    [StructLayout(LayoutKind.Sequential)] internal struct UnicodeString { public ushort length, maximum; public nint buffer; }
    [StructLayout(LayoutKind.Sequential)] struct S4uLogon { public uint type, flags; public UnicodeString user, domain; }
    [StructLayout(LayoutKind.Sequential)] struct Luid { public uint low; public int high; }
    [StructLayout(LayoutKind.Sequential)] struct PrivilegeSet { public uint count, control; public Luid luid; public uint attributes; }
    [StructLayout(LayoutKind.Sequential)] struct TokenSource { [MarshalAs(UnmanagedType.ByValArray, SizeConst = 8)] public byte[] name; public Luid id; }
    [StructLayout(LayoutKind.Sequential)] struct Quotas { public nuint paged, nonPaged, minimum, maximum, pagefile; public long time; }
    [StructLayout(LayoutKind.Sequential)] struct UserInfo1 { public nint name, password; public uint age, privilege; public nint home, comment; public uint flags; public nint script; }
    [StructLayout(LayoutKind.Sequential)] internal struct ProcessInfo { public nint process, thread; public uint pid, tid; }
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)] internal struct StartupInfo
    {
        public int size; public nint reserved, desktop, title; public uint x, y, xSize, ySize, xChars, yChars, fill, flags;
        public ushort show, reservedSize; public nint reservedBytes, stdin, stdout, stderr;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct BasicLimits { public long processTime, jobTime; public uint flags; public nuint minimum, maximum; public uint activeLimit; public nuint affinity; public uint priority, scheduling; }
    [StructLayout(LayoutKind.Sequential)] internal struct JobLimits { public BasicLimits basic; public ulong readOps, writeOps, otherOps, readBytes, writeBytes, otherBytes; public nuint processMemory, jobMemory, peakProcess, peakJob; }

    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool LookupPrivilegeValueW(string? system, string name, out Luid luid);
    [DllImport("advapi32", SetLastError = true)] static extern bool PrivilegeCheck(nint token, ref PrivilegeSet set, out bool result);
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool LookupPrivilegeNameW(string? system, ref Luid luid, StringBuilder name, ref int length);
    [DllImport("advapi32", SetLastError = true)] static extern bool GetTokenInformation(nint token, int kind, nint data, int length, out int needed);
    [DllImport("advapi32", SetLastError = true)] static extern bool OpenProcessToken(nint process, uint access, out nint token);
    [DllImport("advapi32", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcessAsUserW(nint token, string app, StringBuilder command, nint pa, nint ta, bool inherit, uint flags, nint environment, string directory, ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)] static extern bool CreateProcessW(string app, StringBuilder command, nint pa, nint ta, bool inherit, uint flags, nint environment, string directory, ref StartupInfo startup, out ProcessInfo process);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool CloseHandle(nint handle);
    [DllImport("kernel32", SetLastError = true)] internal static extern nint GetStdHandle(int kind);
    [DllImport("kernel32", SetLastError = true)] internal static extern uint ResumeThread(nint thread);
    [DllImport("kernel32", SetLastError = true)] internal static extern uint WaitForSingleObject(nint handle, uint milliseconds);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool GetExitCodeProcess(nint process, out uint code);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool TerminateProcess(nint process, uint code);
    [DllImport("kernel32", SetLastError = true)] internal static extern nint OpenProcess(uint access, bool inherit, uint pid);
    [DllImport("kernel32", CharSet = CharSet.Unicode, SetLastError = true)] internal static extern nint CreateJobObjectW(nint attributes, string? name);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool SetInformationJobObject(nint job, int kind, ref JobLimits data, int length);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool AssignProcessToJobObject(nint job, nint process);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool IsProcessInJob(nint process, nint job, out bool result);
    [DllImport("kernel32", SetLastError = true)] internal static extern bool TerminateJobObject(nint job, uint code);
    [DllImport("kernel32", SetLastError = true)] static extern bool QueryInformationJobObject(nint job, int kind, nint data, int length, out int needed);
    [DllImport("advapi32", SetLastError = true)] static extern bool AllocateLocallyUniqueId(out Luid luid);
    [DllImport("secur32")] static extern uint LsaRegisterLogonProcess(ref UnicodeString name, out nint lsa, out uint mode);
    [DllImport("secur32")] static extern uint LsaLookupAuthenticationPackage(nint lsa, ref UnicodeString name, out uint id);
    [DllImport("secur32")] static extern uint LsaLogonUser(nint lsa, ref UnicodeString origin, uint logonType, uint package, nint authentication, uint length, nint groups, ref TokenSource source, out nint profile, out uint profileLength, out Luid id, out nint token, out Quotas quotas, out uint subStatus);
    [DllImport("secur32")] static extern uint LsaFreeReturnBuffer(nint buffer);
    [DllImport("secur32")] static extern uint LsaDeregisterLogonProcess(nint lsa);
    [DllImport("advapi32")] static extern uint LsaNtStatusToWinError(uint status);
    [DllImport("netapi32", CharSet = CharSet.Unicode)] static extern uint NetUserGetInfo(string? server, string user, uint level, out nint buffer);
    [DllImport("netapi32", CharSet = CharSet.Unicode)] static extern uint NetUserGetLocalGroups(string? server, string user, uint level, uint flags, out nint buffer, uint preferred, out uint read, out uint total);
    [DllImport("netapi32")] static extern uint NetApiBufferFree(nint buffer);
}
