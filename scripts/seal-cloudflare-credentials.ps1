#Requires -Version 7.0
<#
.SYNOPSIS
Seals the owner's Cloudflare DNS bearer token and/or Cloudflare Tunnel credential
JSON into current-user DPAPI ciphertext leaves that arxd's publication profile reads.

.DESCRIPTION
Writes `<TargetDirectory>\dns.dpapi` (profile `dns_ciphertext`) and/or
`<TargetDirectory>\tunnel.dpapi` (profile `tunnel_ciphertext`). The directory is the
profile's `private_root`; arxd requires both leaves directly inside it.

Secrets are read ONLY from:
  * a secure interactive prompt (Read-Host -AsSecureString), the default; or
  * standard input, when -FromStdin is given and stdin is redirected. Exactly one leaf
    per stdin run. The bytes are sealed exactly as received, never trimmed or mutated.
    A DNS token with a trailing CR/LF is refused (arxd refuses it too); write the token
    without a newline. Tunnel JSON may carry JSON whitespace (space, tab, CR, LF),
    including a trailing newline, because arxd's serde_json parse accepts it.

There are deliberately no parameters or environment variables for secret values. The
script refuses to run while a PowerShell transcript is active, under an elevated token,
or when stdin is redirected without -FromStdin. Plaintext stays inside the compiled
helper below (never a PowerShell variable), is never printed, and is zeroed after use.
Only a receipt with output paths, ciphertext lengths and ciphertext SHA-256 digests is
written to stdout. Refusals print a fixed reason code on stderr (exit 2).

Validation mirrors arxd byte-for-byte before anything is written:
  * DNS (cloudflare.rs DnsBearer::new, runtime.rs native_dns read limit 512): 1..=512
    bytes, each in [A-Za-z0-9-_.~+/=].
  * Tunnel (connector.rs TunnelCredential::load, read limit 2048): a JSON object with
    case-sensitive keys AccountTag, TunnelSecret, TunnelID and optional Endpoint, no
    unknown or duplicate keys, string values only, no escapes or control characters
    (arxd borrows the strings, so escaped values are refused there as well).
    AccountTag must equal -AccountId (32 lowercase hex), TunnelID must equal -TunnelId
    (TunnelId::parse: 36 bytes, '-' at 8/13/18/23, lowercase hex elsewhere), Endpoint
    must be empty, and TunnelSecret must be canonical standard base64 of 32 bytes.

Custody mirrors publish/windows.rs read_secret and create_private:
  * The target directory must lie strictly under D:\ArxcovoData, pass arxd's LocalPath
    rules (drive-letter path, <= 240 bytes, safe components only, so no '..', ADS ':'
    or '~' short names), and every component from D:\ to the target must be a real
    directory without a reparse point. All of them stay open (no delete sharing) until
    the run ends, so none can be renamed or swapped meanwhile. GetFinalPathNameByHandle
    must return the requested spelling.
  * The target directory must be owned by the current user and carry a protected DACL
    (inheritance disabled) whose entries only allow the current user. -CreateDirectory
    creates a missing final component atomically with exactly that descriptor.
  * Each leaf is created with CREATE_NEW (an existing file, link or reparse point is
    refused, never replaced) and owner O:<user> with protected DACL D:P(A;;FA;;;<user>)
    set in the creation call. CryptProtectData runs with no entropy, no description and
    CRYPTPROTECT_UI_FORBIDDEN in user scope, matching arxd's CryptUnprotectData call.
  * Before the write handle closes, the ciphertext is read back through it, compared,
    decrypted and compared to the validated plaintext; the link count must be 1. After
    close the leaf is reopened like read_secret (reparse-aware, link count 1, owner and
    private DACL, <= 16384 bytes, user-scope DPAPI header, decrypt within the limit) and
    re-validated. A failure before close deletes the new leaf through its own handle.

This helper never edits daemon configuration, never enables allow_agent_public_sites,
and never contacts Cloudflare. Public publishing stays default-off.

.EXAMPLE
pwsh -NoProfile -File scripts/seal-cloudflare-credentials.ps1 -Mode Both `
  -TargetDirectory D:\ArxcovoData\publish-secrets -CreateDirectory `
  -AccountId <32-hex-account> -TunnelId <tunnel-uuid>
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Dns', 'Tunnel', 'Both')]
    [string]$Mode,

    [Parameter(Mandatory = $true)]
    [string]$TargetDirectory,

    [string]$AccountId,

    [string]$TunnelId,

    [switch]$FromStdin,

    [switch]$CreateDirectory
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$source = @'
using System;
using System.IO;
using System.Runtime.InteropServices;
using System.Security;
using System.Security.AccessControl;
using System.Security.Cryptography;
using System.Security.Principal;
using System.Text;
using Microsoft.Win32.SafeHandles;

namespace Arxcovo.CloudflareSeal
{
    public sealed class SealRefusedException : Exception
    {
        public readonly string Code;
        public SealRefusedException(string code) : base(code) { Code = code; }
    }

    internal static class Native
    {
        public const uint GENERIC_READ = 0x80000000;
        public const uint GENERIC_WRITE = 0x40000000;
        public const uint DELETE = 0x00010000;
        public const uint READ_CONTROL = 0x00020000;
        public const uint FILE_SHARE_READ = 1;
        public const uint FILE_SHARE_WRITE = 2;
        public const uint FILE_SHARE_DELETE = 4;
        public const uint CREATE_NEW = 1;
        public const uint OPEN_EXISTING = 3;
        public const uint FILE_ATTRIBUTE_DIRECTORY = 0x10;
        public const uint FILE_ATTRIBUTE_NORMAL = 0x80;
        public const uint FILE_ATTRIBUTE_REPARSE_POINT = 0x400;
        public const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
        public const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
        public const uint OWNER_SECURITY_INFORMATION = 1;
        public const uint DACL_SECURITY_INFORMATION = 4;
        public const uint CRYPTPROTECT_UI_FORBIDDEN = 1;
        public const int ERROR_FILE_NOT_FOUND = 2;
        public const int ERROR_PATH_NOT_FOUND = 3;

        [StructLayout(LayoutKind.Sequential)]
        public struct SECURITY_ATTRIBUTES
        {
            public int nLength;
            public IntPtr lpSecurityDescriptor;
            public int bInheritHandle;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct BY_HANDLE_FILE_INFORMATION
        {
            public uint dwFileAttributes;
            public uint ftCreationTimeLow, ftCreationTimeHigh;
            public uint ftLastAccessTimeLow, ftLastAccessTimeHigh;
            public uint ftLastWriteTimeLow, ftLastWriteTimeHigh;
            public uint dwVolumeSerialNumber;
            public uint nFileSizeHigh;
            public uint nFileSizeLow;
            public uint nNumberOfLinks;
            public uint nFileIndexHigh;
            public uint nFileIndexLow;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct DATA_BLOB
        {
            public int cbData;
            public IntPtr pbData;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct FILE_DISPOSITION_INFO
        {
            public byte DeleteFile;
        }

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern SafeFileHandle CreateFileW(string name, uint access, uint share,
            IntPtr security, uint disposition, uint flags, IntPtr template);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern SafeFileHandle CreateFileW(string name, uint access, uint share,
            ref SECURITY_ATTRIBUTES security, uint disposition, uint flags, IntPtr template);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CreateDirectoryW(string name, ref SECURITY_ATTRIBUTES security);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetFileAttributesW(string name);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetDriveTypeW(string root);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetFileInformationByHandle(SafeFileHandle file,
            out BY_HANDLE_FILE_INFORMATION info);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern uint GetFinalPathNameByHandleW(SafeFileHandle file,
            StringBuilder path, uint length, uint flags);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool FlushFileBuffers(SafeFileHandle file);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetFileInformationByHandle(SafeFileHandle file, int infoClass,
            ref FILE_DISPOSITION_INFO info, uint size);

        [DllImport("kernel32.dll")]
        public static extern IntPtr LocalFree(IntPtr memory);

        [DllImport("advapi32.dll", SetLastError = true)]
        public static extern uint GetSecurityInfo(SafeFileHandle handle, int objectType,
            uint information, out IntPtr owner, out IntPtr group, out IntPtr dacl,
            out IntPtr sacl, out IntPtr descriptor);

        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool ConvertSecurityDescriptorToStringSecurityDescriptorW(
            IntPtr descriptor, uint revision, uint information, out IntPtr text, out uint length);

        [DllImport("advapi32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool ConvertStringSecurityDescriptorToSecurityDescriptorW(
            string text, uint revision, out IntPtr descriptor, IntPtr size);

        [DllImport("crypt32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CryptProtectData(ref DATA_BLOB input, IntPtr description,
            IntPtr entropy, IntPtr reserved, IntPtr prompt, uint flags, out DATA_BLOB output);

        [DllImport("crypt32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CryptUnprotectData(ref DATA_BLOB input, IntPtr description,
            IntPtr entropy, IntPtr reserved, IntPtr prompt, uint flags, out DATA_BLOB output);
    }

    public sealed class LeafReceipt
    {
        public string Kind;
        public string Path;
        public int CiphertextBytes;
        public string CiphertextSha256;
    }

    /// One validated plaintext leaf. The bytes never leave this object and are zeroed on Dispose.
    public sealed class LeafPlan : IDisposable
    {
        internal readonly string Kind;
        internal readonly string LeafName;
        internal readonly int Limit;
        internal readonly string Account;
        internal readonly string Tunnel;
        internal byte[] Plaintext;

        internal LeafPlan(string kind, byte[] plaintext, string account, string tunnel)
        {
            Kind = kind;
            Account = account;
            Tunnel = tunnel;
            Plaintext = plaintext;
            if (kind == "dns") { LeafName = "dns.dpapi"; Limit = Sealer.DnsLimit; }
            else { LeafName = "tunnel.dpapi"; Limit = Sealer.TunnelLimit; }
        }

        public void Dispose()
        {
            if (Plaintext != null) { Array.Clear(Plaintext, 0, Plaintext.Length); Plaintext = null; }
        }

        public override string ToString() { return "LeafPlan(" + Kind + ", [redacted])"; }
    }

    /// The fenced target directory and its whole retained ancestor chain.
    public sealed class Target : IDisposable
    {
        internal readonly string Directory;
        internal readonly SafeFileHandle[] Chain;
        internal Target(string directory, SafeFileHandle[] chain) { Directory = directory; Chain = chain; }
        public string Path { get { return Directory; } }
        public void Dispose()
        {
            if (Chain == null) return;
            for (int i = Chain.Length - 1; i >= 0; i--) { if (Chain[i] != null) Chain[i].Dispose(); }
        }
    }

    public static class Sealer
    {
        public const int DnsLimit = 512;
        public const int TunnelLimit = 2048;
        public const int CiphertextLimit = 16384;
        public const string Root = @"D:\ArxcovoData";

        static SealRefusedException Refuse(string code) { return new SealRefusedException(code); }

        static SecurityIdentifier User()
        {
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) { return identity.User; }
        }

        // ---- identifiers (publish/mod.rs hex_id, TunnelId::parse) -----------------------------

        static bool LowerHex(char c) { return (c >= '0' && c <= '9') || (c >= 'a' && c <= 'f'); }

        public static bool HexId(string value)
        {
            if (value == null || value.Length != 32) return false;
            foreach (char c in value) if (!LowerHex(c)) return false;
            return true;
        }

        public static bool TunnelIdValid(string value)
        {
            if (value == null || value.Length != 36) return false;
            for (int i = 0; i < 36; i++)
            {
                char c = value[i];
                if (i == 8 || i == 13 || i == 18 || i == 23) { if (c != '-') return false; }
                else if (!LowerHex(c)) return false;
            }
            return true;
        }

        // ---- payload rules ---------------------------------------------------------------------

        /// cloudflare.rs DnsBearer::new after read_secret(path, 512) and from_utf8.
        public static string CheckDns(byte[] value)
        {
            if (value.Length == 0) return "dns-token-empty";
            if (value.Length > DnsLimit) return "dns-token-too-long";
            foreach (byte b in value)
            {
                bool ok = (b >= (byte)'A' && b <= (byte)'Z') || (b >= (byte)'a' && b <= (byte)'z')
                    || (b >= (byte)'0' && b <= (byte)'9') || b == (byte)'-' || b == (byte)'_'
                    || b == (byte)'.' || b == (byte)'~' || b == (byte)'+' || b == (byte)'/'
                    || b == (byte)'=';
                if (!ok) return "dns-token-forbidden-byte";
            }
            return null;
        }

        static bool JsonSpace(byte b) { return b == 0x20 || b == 0x09 || b == 0x0A || b == 0x0D; }

        static string ReadString(byte[] v, ref int i, out string value)
        {
            value = null;
            if (i >= v.Length || v[i] != (byte)'"') return "tunnel-json-non-string-value";
            i++;
            int start = i;
            while (i < v.Length && v[i] != (byte)'"')
            {
                byte b = v[i];
                if (b == (byte)'\\') return "tunnel-json-escape";
                if (b < 0x20 || b > 0x7E) return "tunnel-json-forbidden-byte";
                i++;
            }
            if (i >= v.Length) return "tunnel-json-malformed";
            value = Encoding.ASCII.GetString(v, start, i - start);
            i++;
            return null;
        }

        static void SkipSpace(byte[] v, ref int i) { while (i < v.Length && JsonSpace(v[i])) i++; }

        /// connector.rs CredentialJson + TunnelCredential::load after read_secret(path, 2048).
        public static string CheckTunnel(byte[] v, string account, string tunnel)
        {
            if (v.Length == 0) return "tunnel-json-empty";
            if (v.Length > TunnelLimit) return "tunnel-json-too-long";
            if (v.Length >= 3 && v[0] == 0xEF && v[1] == 0xBB && v[2] == 0xBF) return "tunnel-json-bom";
            string accountTag = null, secret = null, tunnelId = null, endpoint = null;
            int i = 0;
            SkipSpace(v, ref i);
            if (i >= v.Length || v[i] != (byte)'{') return "tunnel-json-not-object";
            i++;
            SkipSpace(v, ref i);
            if (i < v.Length && v[i] == (byte)'}') return "tunnel-json-missing-field";
            while (true)
            {
                string key, value, error;
                if (i >= v.Length || v[i] != (byte)'"') return "tunnel-json-malformed";
                error = ReadString(v, ref i, out key);
                if (error != null) return error;
                SkipSpace(v, ref i);
                if (i >= v.Length || v[i] != (byte)':') return "tunnel-json-malformed";
                i++;
                SkipSpace(v, ref i);
                error = ReadString(v, ref i, out value);
                if (error != null) return error;
                switch (key)
                {
                    case "AccountTag": if (accountTag != null) return "tunnel-json-duplicate-field"; accountTag = value; break;
                    case "TunnelSecret": if (secret != null) return "tunnel-json-duplicate-field"; secret = value; break;
                    case "TunnelID": if (tunnelId != null) return "tunnel-json-duplicate-field"; tunnelId = value; break;
                    case "Endpoint": if (endpoint != null) return "tunnel-json-duplicate-field"; endpoint = value; break;
                    default: return "tunnel-json-unknown-field";
                }
                SkipSpace(v, ref i);
                if (i < v.Length && v[i] == (byte)',') { i++; SkipSpace(v, ref i); continue; }
                if (i < v.Length && v[i] == (byte)'}') { i++; break; }
                return "tunnel-json-malformed";
            }
            SkipSpace(v, ref i);
            if (i != v.Length) return "tunnel-json-trailing-data";
            if (accountTag == null || secret == null || tunnelId == null) return "tunnel-json-missing-field";
            if (!HexId(accountTag)) return "tunnel-account-tag-invalid";
            if (!string.Equals(accountTag, account, StringComparison.Ordinal)) return "tunnel-account-tag-mismatch";
            if (!TunnelIdValid(tunnelId)) return "tunnel-id-invalid";
            if (!string.Equals(tunnelId, tunnel, StringComparison.Ordinal)) return "tunnel-id-mismatch";
            if (endpoint != null && endpoint.Length != 0) return "tunnel-endpoint-not-empty";
            if (secret.Length != 44 || secret[43] != '=') return "tunnel-secret-not-canonical";
            for (int k = 0; k < 43; k++)
            {
                char c = secret[k];
                bool ok = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')
                    || c == '+' || c == '/';
                if (!ok) return "tunnel-secret-not-canonical";
            }
            if ("AEIMQUYcgkosw048".IndexOf(secret[42]) < 0) return "tunnel-secret-not-canonical";
            return null;
        }

        static void CheckIdentifiers(string kind, string account, string tunnel)
        {
            if (kind != "tunnel") return;
            if (!HexId(account)) throw Refuse("account-id-invalid");
            if (!TunnelIdValid(tunnel)) throw Refuse("tunnel-id-argument-invalid");
        }

        static LeafPlan Validated(string kind, byte[] plaintext, string account, string tunnel)
        {
            string error = kind == "dns" ? CheckDns(plaintext) : CheckTunnel(plaintext, account, tunnel);
            if (error != null)
            {
                Array.Clear(plaintext, 0, plaintext.Length);
                throw Refuse(error);
            }
            return new LeafPlan(kind, plaintext, account, tunnel);
        }

        static string NormalizeKind(string kind)
        {
            if (kind == "Dns") return "dns";
            if (kind == "Tunnel") return "tunnel";
            throw Refuse("mode-invalid");
        }

        // ---- secret input ----------------------------------------------------------------------

        /// Reads one leaf from redirected stdin, refusing more than the leaf's limit.
        public static LeafPlan FromStdin(string kindName, string account, string tunnel)
        {
            string kind = NormalizeKind(kindName);
            CheckIdentifiers(kind, account, tunnel);
            if (!Console.IsInputRedirected) throw Refuse("stdin-not-redirected");
            int limit = kind == "dns" ? DnsLimit : TunnelLimit;
            byte[] buffer = new byte[limit + 1];
            int total = 0;
            try
            {
                using (Stream input = Console.OpenStandardInput())
                {
                    while (total < buffer.Length)
                    {
                        int read = input.Read(buffer, total, buffer.Length - total);
                        if (read <= 0) break;
                        total += read;
                    }
                }
                if (total > limit) throw Refuse(kind == "dns" ? "dns-token-too-long" : "tunnel-json-too-long");
                byte[] exact = new byte[total];
                Buffer.BlockCopy(buffer, 0, exact, 0, total);
                return Validated(kind, exact, account, tunnel);
            }
            finally
            {
                Array.Clear(buffer, 0, buffer.Length);
            }
        }

        /// Converts one interactive SecureString entry (UTF-16) to its exact ASCII bytes.
        public static LeafPlan FromSecureString(string kindName, SecureString value, string account, string tunnel)
        {
            string kind = NormalizeKind(kindName);
            CheckIdentifiers(kind, account, tunnel);
            int limit = kind == "dns" ? DnsLimit : TunnelLimit;
            if (value == null || value.Length == 0) throw Refuse(kind == "dns" ? "dns-token-empty" : "tunnel-json-empty");
            if (value.Length > limit) throw Refuse(kind == "dns" ? "dns-token-too-long" : "tunnel-json-too-long");
            IntPtr raw = Marshal.SecureStringToGlobalAllocUnicode(value);
            byte[] bytes = new byte[value.Length];
            try
            {
                for (int i = 0; i < bytes.Length; i++)
                {
                    int c = (ushort)Marshal.ReadInt16(raw, 2 * i);
                    if (c > 0x7F)
                    {
                        Array.Clear(bytes, 0, bytes.Length);
                        throw Refuse(kind == "dns" ? "dns-token-forbidden-byte" : "tunnel-json-forbidden-byte");
                    }
                    bytes[i] = (byte)c;
                }
            }
            finally
            {
                Marshal.ZeroFreeGlobalAllocUnicode(raw);
            }
            return Validated(kind, bytes, account, tunnel);
        }

        // ---- path fence ------------------------------------------------------------------------

        /// paths.rs safe_component.
        static bool SafeComponent(string name)
        {
            if (name.Length == 0 || name.EndsWith(".") || name.EndsWith(" ")) return false;
            foreach (char c in name)
            {
                bool ok = (c >= 'A' && c <= 'Z') || (c >= 'a' && c <= 'z') || (c >= '0' && c <= '9')
                    || c == ' ' || c == '.' || c == '-' || c == '_';
                if (!ok) return false;
            }
            string stem = name.Split('.')[0].TrimEnd(' ').ToUpperInvariant();
            if (stem == "CON" || stem == "PRN" || stem == "AUX" || stem == "NUL") return false;
            if (stem.Length == 4 && (stem.StartsWith("COM") || stem.StartsWith("LPT")) && char.IsDigit(stem[3])
                && stem[3] <= '9') return false;
            return true;
        }

        /// Returns the normalized absolute target directory or refuses.
        public static string FencePath(string raw)
        {
            if (raw == null || raw.Length < 4 || raw.Length > 240) throw Refuse("path-invalid");
            char drive = raw[0];
            bool letter = (drive >= 'A' && drive <= 'Z') || (drive >= 'a' && drive <= 'z');
            if (!letter || raw[1] != ':' || (raw[2] != '\\' && raw[2] != '/')) throw Refuse("path-invalid");
            string normalized = raw.Replace('/', '\\');
            string[] parts = normalized.Substring(3).Split('\\');
            foreach (string part in parts) if (!SafeComponent(part)) throw Refuse("path-invalid");
            if (char.ToUpperInvariant(drive) != 'D' || parts.Length < 2
                || !string.Equals(parts[0], "ArxcovoData", StringComparison.OrdinalIgnoreCase))
                throw Refuse("path-outside-arxcovodata");
            string result = "D:\\" + string.Join("\\", parts);
            // Both leaves must also satisfy LocalPath's 240-byte bound.
            if ((result + "\\tunnel.dpapi").Length > 240) throw Refuse("path-invalid");
            return result;
        }

        static SafeFileHandle OpenDirectory(string path, bool forSecurity)
        {
            SafeFileHandle handle = Native.CreateFileW(path, Native.GENERIC_READ,
                Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE, IntPtr.Zero, Native.OPEN_EXISTING,
                Native.FILE_FLAG_BACKUP_SEMANTICS | Native.FILE_FLAG_OPEN_REPARSE_POINT, IntPtr.Zero);
            if (handle.IsInvalid)
            {
                int error = Marshal.GetLastWin32Error();
                handle.Dispose();
                if (error == Native.ERROR_FILE_NOT_FOUND || error == Native.ERROR_PATH_NOT_FOUND)
                    throw Refuse(forSecurity ? "target-directory-missing" : "ancestor-missing");
                throw Refuse("ancestor-unopenable");
            }
            Native.BY_HANDLE_FILE_INFORMATION info;
            if (!Native.GetFileInformationByHandle(handle, out info)) { handle.Dispose(); throw Refuse("ancestor-unreadable"); }
            if ((info.dwFileAttributes & Native.FILE_ATTRIBUTE_REPARSE_POINT) != 0)
            {
                handle.Dispose();
                throw Refuse("reparse-point-in-path");
            }
            if ((info.dwFileAttributes & Native.FILE_ATTRIBUTE_DIRECTORY) == 0)
            {
                handle.Dispose();
                throw Refuse("ancestor-not-directory");
            }
            return handle;
        }

        static RawSecurityDescriptor Describe(SafeFileHandle handle)
        {
            IntPtr owner, group, dacl, sacl, descriptor;
            uint status = Native.GetSecurityInfo(handle, 1,
                Native.OWNER_SECURITY_INFORMATION | Native.DACL_SECURITY_INFORMATION,
                out owner, out group, out dacl, out sacl, out descriptor);
            if (status != 0) throw Refuse("security-unreadable");
            try
            {
                IntPtr text;
                uint length;
                if (!Native.ConvertSecurityDescriptorToStringSecurityDescriptorW(descriptor, 1,
                        Native.OWNER_SECURITY_INFORMATION | Native.DACL_SECURITY_INFORMATION, out text, out length))
                    throw Refuse("security-unreadable");
                try { return new RawSecurityDescriptor(Marshal.PtrToStringUni(text)); }
                finally { Native.LocalFree(text); }
            }
            finally { Native.LocalFree(descriptor); }
        }

        /// Owner is the current user; protected DACL; only allow entries for the current user.
        static bool OwnerPrivate(SafeFileHandle handle)
        {
            SecurityIdentifier user = User();
            RawSecurityDescriptor sd = Describe(handle);
            if (sd.Owner == null || !sd.Owner.Equals(user)) return false;
            if ((sd.ControlFlags & ControlFlags.DiscretionaryAclPresent) == 0) return false;
            if ((sd.ControlFlags & ControlFlags.DiscretionaryAclProtected) == 0) return false;
            RawAcl acl = sd.DiscretionaryAcl;
            if (acl == null || acl.Count == 0) return false;
            for (int i = 0; i < acl.Count; i++)
            {
                CommonAce ace = acl[i] as CommonAce;
                if (ace == null || ace.AceQualifier != AceQualifier.AccessAllowed) return false;
                if (!ace.SecurityIdentifier.Equals(user)) return false;
                if ((ace.AceFlags & AceFlags.Inherited) != 0) return false;
            }
            return true;
        }

        /// publish/windows.rs create_private: owner and a protected DACL granting only TokenUser.
        /// A created directory's single entry is also inheritable (OICI) so later children stay private.
        static string PrivateSddl(bool directory)
        {
            string sid = User().Value;
            return "O:" + sid + "D:P(A;" + (directory ? "OICI" : "") + ";FA;;;" + sid + ")";
        }

        static IntPtr PrivateDescriptor(bool directory)
        {
            IntPtr descriptor;
            if (!Native.ConvertStringSecurityDescriptorToSecurityDescriptorW(PrivateSddl(directory), 1, out descriptor, IntPtr.Zero))
                throw Refuse("security-descriptor-failed");
            return descriptor;
        }

        static string FinalPath(SafeFileHandle handle)
        {
            StringBuilder buffer = new StringBuilder(1024);
            uint length = Native.GetFinalPathNameByHandleW(handle, buffer, (uint)buffer.Capacity, 0);
            if (length == 0 || length >= buffer.Capacity) throw Refuse("final-path-unreadable");
            return buffer.ToString();
        }

        /// Fences, optionally creates the final component, and retains the whole chain.
        public static Target Prepare(string rawDirectory, bool create)
        {
            string directory = FencePath(rawDirectory);
            if (Native.GetDriveTypeW("D:\\") != 3) throw Refuse("drive-not-fixed");
            string[] parts = directory.Substring(3).Split('\\');
            SafeFileHandle[] chain = new SafeFileHandle[parts.Length + 1];
            bool done = false;
            try
            {
                chain[0] = OpenDirectory("D:\\", false);
                string current = "D:";
                for (int i = 0; i < parts.Length; i++)
                {
                    current = current + "\\" + parts[i];
                    bool last = i == parts.Length - 1;
                    if (last && create && Native.GetFileAttributesW(current) == 0xFFFFFFFF
                        && Marshal.GetLastWin32Error() == Native.ERROR_FILE_NOT_FOUND)
                    {
                        IntPtr descriptor = PrivateDescriptor(true);
                        try
                        {
                            Native.SECURITY_ATTRIBUTES attributes = new Native.SECURITY_ATTRIBUTES();
                            attributes.nLength = Marshal.SizeOf(typeof(Native.SECURITY_ATTRIBUTES));
                            attributes.lpSecurityDescriptor = descriptor;
                            if (!Native.CreateDirectoryW(current, ref attributes)) throw Refuse("target-directory-create-failed");
                        }
                        finally { Native.LocalFree(descriptor); }
                    }
                    chain[i + 1] = OpenDirectory(current, last);
                }
                SafeFileHandle target = chain[chain.Length - 1];
                if (!string.Equals(FinalPath(target), "\\\\?\\" + directory, StringComparison.OrdinalIgnoreCase))
                    throw Refuse("final-path-mismatch");
                if (!OwnerPrivate(target)) throw Refuse("target-directory-not-private");
                done = true;
                return new Target(directory, chain);
            }
            finally
            {
                if (!done) foreach (SafeFileHandle handle in chain) if (handle != null) handle.Dispose();
            }
        }

        public static void EnsureAbsent(Target target, string kindName)
        {
            string leaf = NormalizeKind(kindName) == "dns" ? "dns.dpapi" : "tunnel.dpapi";
            string path = target.Directory + "\\" + leaf;
            if (Native.GetFileAttributesW(path) != 0xFFFFFFFF) throw Refuse("target-file-exists");
            if (Marshal.GetLastWin32Error() != Native.ERROR_FILE_NOT_FOUND) throw Refuse("target-file-unknown");
        }

        // ---- DPAPI -----------------------------------------------------------------------------

        static byte[] Protect(byte[] plaintext)
        {
            GCHandle pin = GCHandle.Alloc(plaintext, GCHandleType.Pinned);
            try
            {
                Native.DATA_BLOB input = new Native.DATA_BLOB();
                input.cbData = plaintext.Length;
                input.pbData = pin.AddrOfPinnedObject();
                Native.DATA_BLOB output;
                if (!Native.CryptProtectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero,
                        Native.CRYPTPROTECT_UI_FORBIDDEN, out output) || output.pbData == IntPtr.Zero)
                    throw Refuse("dpapi-protect-failed");
                try
                {
                    byte[] result = new byte[output.cbData];
                    Marshal.Copy(output.pbData, result, 0, output.cbData);
                    return result;
                }
                finally { Native.LocalFree(output.pbData); }
            }
            finally { pin.Free(); }
        }

        static byte[] Unprotect(byte[] ciphertext, int limit)
        {
            GCHandle pin = GCHandle.Alloc(ciphertext, GCHandleType.Pinned);
            try
            {
                Native.DATA_BLOB input = new Native.DATA_BLOB();
                input.cbData = ciphertext.Length;
                input.pbData = pin.AddrOfPinnedObject();
                Native.DATA_BLOB output;
                if (!Native.CryptUnprotectData(ref input, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero, IntPtr.Zero,
                        Native.CRYPTPROTECT_UI_FORBIDDEN, out output) || output.pbData == IntPtr.Zero)
                    throw Refuse("dpapi-unprotect-failed");
                try
                {
                    if (output.cbData <= 0 || output.cbData > limit) throw Refuse("dpapi-roundtrip-length");
                    byte[] result = new byte[output.cbData];
                    Marshal.Copy(output.pbData, result, 0, output.cbData);
                    return result;
                }
                finally
                {
                    for (int i = 0; i < output.cbData; i++) Marshal.WriteByte(output.pbData, i, 0);
                    Native.LocalFree(output.pbData);
                }
            }
            finally { pin.Free(); }
        }

        /// security.rs user_scope_dpapi: version 1, default provider, no CRYPTPROTECT_LOCAL_MACHINE.
        public static bool UserScopeDpapi(byte[] blob)
        {
            byte[] provider = { 0xd0, 0x8c, 0x9d, 0xdf, 0x01, 0x15, 0xd1, 0x11, 0x8c, 0x7a, 0x00, 0xc0, 0x4f, 0xc2, 0x97, 0xeb };
            if (blob.Length < 44) return false;
            if (BitConverter.ToUInt32(blob, 0) != 1) return false;
            for (int i = 0; i < 16; i++) if (blob[4 + i] != provider[i]) return false;
            return (BitConverter.ToUInt32(blob, 40) & 0x4) == 0;
        }

        static bool Same(byte[] a, byte[] b)
        {
            return CryptographicOperations.FixedTimeEquals(a, b);
        }

        /// Marks a leaf this run created for deletion through its own handle (no by-path delete).
        static bool Delete(SafeFileHandle handle)
        {
            Native.FILE_DISPOSITION_INFO info = new Native.FILE_DISPOSITION_INFO();
            info.DeleteFile = 1;
            return Native.SetFileInformationByHandle(handle, 4, ref info, 1);
        }

        static void CheckLeafHandle(SafeFileHandle handle)
        {
            Native.BY_HANDLE_FILE_INFORMATION info;
            if (!Native.GetFileInformationByHandle(handle, out info)) throw Refuse("leaf-unreadable");
            if ((info.dwFileAttributes & (Native.FILE_ATTRIBUTE_REPARSE_POINT | Native.FILE_ATTRIBUTE_DIRECTORY)) != 0)
                throw Refuse("leaf-not-plain-file");
            if (info.nNumberOfLinks != 1) throw Refuse("leaf-link-count");
        }

        /// Bounded read through a handle that stays owned by the caller.
        static byte[] ReadAll(SafeFileHandle handle, string tooLarge)
        {
            byte[] buffer = new byte[CiphertextLimit + 1];
            int total = 0, read;
            while (total < buffer.Length
                && (read = RandomAccess.Read(handle, new Span<byte>(buffer, total, buffer.Length - total), total)) > 0)
                total += read;
            if (total > CiphertextLimit) throw Refuse(tooLarge);
            byte[] exact = new byte[total];
            Buffer.BlockCopy(buffer, 0, exact, 0, total);
            return exact;
        }

        // ---- sealing ---------------------------------------------------------------------------

        public static LeafReceipt Seal(Target target, LeafPlan plan)
        {
            if (plan == null || plan.Plaintext == null) throw Refuse("plan-consumed");
            string path = target.Directory + "\\" + plan.LeafName;
            byte[] ciphertext = Protect(plan.Plaintext);
            if (ciphertext.Length > CiphertextLimit || !UserScopeDpapi(ciphertext)) throw Refuse("dpapi-blob-unsupported");
            IntPtr descriptor = PrivateDescriptor(false);
            SafeFileHandle file;
            try
            {
                Native.SECURITY_ATTRIBUTES attributes = new Native.SECURITY_ATTRIBUTES();
                attributes.nLength = Marshal.SizeOf(typeof(Native.SECURITY_ATTRIBUTES));
                attributes.lpSecurityDescriptor = descriptor;
                // CREATE_NEW never opens, follows or replaces an existing name. Read sharing only
                // admits the parity reader below while this handle keeps write and delete access.
                file = Native.CreateFileW(path, Native.GENERIC_READ | Native.GENERIC_WRITE | Native.DELETE,
                    Native.FILE_SHARE_READ, ref attributes, Native.CREATE_NEW,
                    Native.FILE_ATTRIBUTE_NORMAL | Native.FILE_FLAG_OPEN_REPARSE_POINT, IntPtr.Zero);
            }
            finally { Native.LocalFree(descriptor); }
            if (file.IsInvalid)
            {
                file.Dispose();
                throw Refuse("target-file-create-failed");
            }
            using (file)
            {
                try
                {
                    CheckLeafHandle(file);
                    if (!OwnerPrivate(file)) throw Refuse("leaf-not-private");
                    RandomAccess.Write(file, new ReadOnlySpan<byte>(ciphertext), 0);
                    if (!Native.FlushFileBuffers(file)) throw Refuse("leaf-flush-failed");
                    byte[] back = ReadAll(file, "readback-mismatch");
                    if (!Same(back, ciphertext)) throw Refuse("readback-mismatch");
                    byte[] plain = Unprotect(back, plan.Limit);
                    try
                    {
                        if (!Same(plain, plan.Plaintext)) throw Refuse("readback-plaintext-mismatch");
                    }
                    finally { Array.Clear(plain, 0, plain.Length); }
                    VerifyLikeReadSecret(target, plan, ciphertext);
                }
                catch (SealRefusedException refused)
                {
                    if (!Delete(file)) throw Refuse(refused.Code + "-residue-unremoved");
                    throw;
                }
                catch
                {
                    if (!Delete(file)) throw Refuse("leaf-residue-unremoved");
                    throw;
                }
            }
            LeafReceipt receipt = new LeafReceipt();
            receipt.Kind = plan.Kind;
            receipt.Path = path;
            receipt.CiphertextBytes = ciphertext.Length;
            receipt.CiphertextSha256 = Convert.ToHexString(SHA256.HashData(ciphertext)).ToLowerInvariant();
            return receipt;
        }

        /// Opens the leaf again by path the way publish/windows.rs read_secret does (reparse-aware
        /// open, plain file, one link, owner and private DACL, bounded ciphertext, user-scope DPAPI
        /// header, decrypt within the read limit) and re-validates the decrypted payload.
        static void VerifyLikeReadSecret(Target target, LeafPlan plan, byte[] expected)
        {
            string path = target.Directory + "\\" + plan.LeafName;
            if (!OwnerPrivate(target.Chain[target.Chain.Length - 1])) throw Refuse("parity-directory-not-private");
            using (SafeFileHandle file = Native.CreateFileW(path, Native.GENERIC_READ,
                Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE | Native.FILE_SHARE_DELETE, IntPtr.Zero,
                Native.OPEN_EXISTING, Native.FILE_FLAG_OPEN_REPARSE_POINT | Native.FILE_FLAG_BACKUP_SEMANTICS,
                IntPtr.Zero))
            {
                if (file.IsInvalid) throw Refuse("parity-open-failed");
                CheckLeafHandle(file);
                if (!OwnerPrivate(file)) throw Refuse("parity-leaf-not-private");
                byte[] exact = ReadAll(file, "parity-ciphertext-too-large");
                if (!Same(exact, expected) || !UserScopeDpapi(exact)) throw Refuse("parity-ciphertext-mismatch");
                byte[] plain = Unprotect(exact, plan.Limit);
                try
                {
                    string error = plan.Kind == "dns" ? CheckDns(plain) : CheckTunnel(plain, plan.Account, plan.Tunnel);
                    if (error != null) throw Refuse("parity-" + error);
                }
                finally { Array.Clear(plain, 0, plain.Length); }
            }
        }
    }
}
'@

function Write-Refusal([string]$Code) {
    [Console]::Error.WriteLine("seal-cloudflare-credentials: refused: $Code")
    exit 2
}

if (-not $IsWindows) { Write-Refusal 'windows-required' }

# A transcript would capture the session; refuse rather than trust that nothing secret appears.
$transcribing = [System.Management.Automation.Host.PSHostUserInterface].GetProperty(
    'IsTranscribing', [Reflection.BindingFlags]'NonPublic,Public,Instance')
if ($null -eq $transcribing) { Write-Refusal 'transcript-state-unknown' }
if ($transcribing.GetValue($Host.UI)) { Write-Refusal 'transcript-active' }
foreach ($policy in @(
        'HKLM:\SOFTWARE\Policies\Microsoft\PowerShellCore\Transcription',
        'HKCU:\SOFTWARE\Policies\Microsoft\PowerShellCore\Transcription')) {
    $value = Get-ItemProperty -LiteralPath $policy -Name EnableTranscripting -ErrorAction SilentlyContinue
    if ($null -ne $value -and $value.EnableTranscripting -eq 1) { Write-Refusal 'transcript-policy-enabled' }
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
try {
    $principal = [Security.Principal.WindowsPrincipal]::new($identity)
    if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
        Write-Refusal 'elevated-token'
    }
} finally {
    $identity.Dispose()
}

$kinds = @(switch ($Mode) {
    'Dns' { @('Dns') }
    'Tunnel' { @('Tunnel') }
    'Both' { @('Dns', 'Tunnel') }
})
if ($kinds -contains 'Tunnel') {
    if ([string]::IsNullOrEmpty($AccountId) -or [string]::IsNullOrEmpty($TunnelId)) {
        Write-Refusal 'tunnel-identifiers-required'
    }
} elseif ($PSBoundParameters.ContainsKey('AccountId') -or $PSBoundParameters.ContainsKey('TunnelId')) {
    Write-Refusal 'tunnel-identifiers-unexpected'
}
if ($FromStdin) {
    if ($kinds.Count -ne 1) { Write-Refusal 'stdin-single-leaf-only' }
    if (-not [Console]::IsInputRedirected) { Write-Refusal 'stdin-not-redirected' }
} elseif ([Console]::IsInputRedirected) {
    # A prompt over redirected input would read the secret from an unknown source.
    Write-Refusal 'stdin-redirected-without-fromstdin'
}

if (-not ('Arxcovo.CloudflareSeal.Sealer' -as [type])) {
    Add-Type -TypeDefinition $source -Language CSharp
}

$target = $null
$plans = [System.Collections.Generic.List[object]]::new()
try {
    try {
        $target = [Arxcovo.CloudflareSeal.Sealer]::Prepare($TargetDirectory, [bool]$CreateDirectory)
        foreach ($kind in $kinds) { [Arxcovo.CloudflareSeal.Sealer]::EnsureAbsent($target, $kind) }

        foreach ($kind in $kinds) {
            if ($FromStdin) {
                $plans.Add([Arxcovo.CloudflareSeal.Sealer]::FromStdin($kind, $AccountId, $TunnelId))
            } else {
                $prompt = if ($kind -eq 'Dns') { 'Cloudflare DNS API token' } else { 'Cloudflare Tunnel credential JSON (one line)' }
                $secure = Read-Host -Prompt $prompt -AsSecureString
                try {
                    $plans.Add([Arxcovo.CloudflareSeal.Sealer]::FromSecureString($kind, $secure, $AccountId, $TunnelId))
                } finally {
                    $secure.Dispose()
                }
            }
        }

        # Every leaf is validated, and absent, before the first write.
        foreach ($kind in $kinds) { [Arxcovo.CloudflareSeal.Sealer]::EnsureAbsent($target, $kind) }
        $leaves = foreach ($plan in $plans) {
            $receipt = [Arxcovo.CloudflareSeal.Sealer]::Seal($target, $plan)
            [ordered]@{
                kind              = $receipt.Kind
                path              = $receipt.Path
                ciphertext_bytes  = $receipt.CiphertextBytes
                ciphertext_sha256 = $receipt.CiphertextSha256
            }
        }
        [ordered]@{
            schema                  = 'arxcovo.cloudflare-seal.v1'
            target_directory        = $target.Path
            dpapi_scope             = 'current_user'
            leaves                  = @($leaves)
            daemon_config_modified  = $false
            public_publishing       = 'unchanged-default-off'
        } | ConvertTo-Json -Depth 4 -Compress | Write-Output
    } catch {
        $inner = $_.Exception
        while ($inner -is [System.Management.Automation.MethodInvocationException] -and $null -ne $inner.InnerException) {
            $inner = $inner.InnerException
        }
        if ($inner -is [Arxcovo.CloudflareSeal.SealRefusedException]) { Write-Refusal $inner.Code }
        # Never print exception messages: they could quote an input path or value.
        [Console]::Error.WriteLine("seal-cloudflare-credentials: failed: $($inner.GetType().FullName)")
        exit 3
    }
} finally {
    foreach ($plan in $plans) { $plan.Dispose() }
    if ($null -ne $target) { $target.Dispose() }
}
