using System;
using System.Runtime.InteropServices;
using System.Security.Principal;
// Invoked only by the guarded disposable-CI entrypoint; no static initializer.
public static class ProbeBatchRight {
    [StructLayout(LayoutKind.Sequential)] struct Attributes { public int Length; public IntPtr Root, Name; public uint Flags; public IntPtr Descriptor, Qos; }
    [StructLayout(LayoutKind.Sequential)] struct Unicode { public ushort Length, MaximumLength; public IntPtr Buffer; }
    [DllImport("advapi32.dll")] static extern uint LsaOpenPolicy(IntPtr system, ref Attributes attributes, uint access, out IntPtr policy);
    [DllImport("advapi32.dll")] static extern uint LsaAddAccountRights(IntPtr policy, byte[] sid, Unicode[] rights, uint count);
    [DllImport("advapi32.dll")] static extern uint LsaRemoveAccountRights(IntPtr policy, byte[] sid, [MarshalAs(UnmanagedType.U1)] bool all, Unicode[] rights, uint count);
    [DllImport("advapi32.dll")] static extern uint LsaEnumerateAccountRights(IntPtr policy, byte[] sid, out IntPtr rights, out uint count);
    [DllImport("advapi32.dll")] static extern uint LsaFreeMemory(IntPtr buffer);
    [DllImport("advapi32.dll")] static extern uint LsaClose(IntPtr policy);
    static void Check(uint status) { if (status != 0) throw new InvalidOperationException("lsa-right-status-" + status); }
    public static void Change(string sidText, bool add) {
        // Defense against accidental direct invocation; these are spoofable intent guards.
        foreach (var pair in new[] { ("GITHUB_ACTIONS", "true"), ("RUNNER_ENVIRONMENT", "github-hosted"),
            ("RUNNER_OS", "Windows"), ("ARXCOVO_DISPOSABLE_PROBE", "reviewed-manual-ci-v1"),
            ("GITHUB_EVENT_NAME", "workflow_dispatch"), ("GITHUB_REPOSITORY", "Dytschgo/arxcovo"),
            ("GITHUB_ACTOR", "Dytschgo"), ("GITHUB_TRIGGERING_ACTOR", "Dytschgo"), ("GITHUB_REF", "refs/heads/main") })
            if (Environment.GetEnvironmentVariable(pair.Item1) != pair.Item2) throw new InvalidOperationException("ci-only-right-refused");
        string reviewed = Environment.GetEnvironmentVariable("REVIEWED_SHA") ?? "";
        if (!System.Text.RegularExpressions.Regex.IsMatch(reviewed, @"\A[0-9a-f]{40}\z") ||
            reviewed != Environment.GetEnvironmentVariable("GITHUB_SHA") || reviewed != Environment.GetEnvironmentVariable("GITHUB_WORKFLOW_SHA"))
            throw new InvalidOperationException("reviewed-source-refused");
        if (!System.Text.RegularExpressions.Regex.IsMatch(sidText, @"\AS-1-5-21-(?:[0-9]+-){3}[0-9]+\z")) throw new InvalidOperationException("sid-refused");
        var sid = new SecurityIdentifier(sidText); byte[] bytes = new byte[sid.BinaryLength]; sid.GetBinaryForm(bytes, 0);
        var attr = new Attributes { Length = Marshal.SizeOf<Attributes>() };
        Check(LsaOpenPolicy(IntPtr.Zero, ref attr, 0x810, out var policy)); // LOOKUP_NAMES | CREATE_ACCOUNT
        IntPtr buffer = IntPtr.Zero;
        try {
            if (add) {
                uint status = LsaEnumerateAccountRights(policy, bytes, out var existing, out uint count);
                try { if (status != 0xc0000034) { Check(status); if (count != 0) throw new InvalidOperationException("existing-rights-refused"); } }
                finally { if (existing != IntPtr.Zero) LsaFreeMemory(existing); }
            }
            buffer = Marshal.StringToHGlobalUni("SeBatchLogonRight");
            var rights = new[] { new Unicode { Length = 34, MaximumLength = 36, Buffer = buffer } };
            Check(add ? LsaAddAccountRights(policy, bytes, rights, 1) : LsaRemoveAccountRights(policy, bytes, false, rights, 1));
        } finally { if (buffer != IntPtr.Zero) Marshal.FreeHGlobal(buffer); LsaClose(policy); }
    }
}
