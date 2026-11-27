#Requires -Version 7.2
<#
.SYNOPSIS
Seals the owner's Cloudflare DNS bearer token and/or Cloudflare Tunnel credential
JSON into current-user DPAPI ciphertext leaves that arxd's publication profile reads.

.DESCRIPTION
Writes `<TargetDirectory>\dns.dpapi` (profile `dns_ciphertext`) and/or
`<TargetDirectory>\tunnel.dpapi` (profile `tunnel_ciphertext`). The directory is the
profile's `private_root`; arxd requires both leaves directly inside it.

Requires PowerShell 7.2 or later (.NET 6, for System.IO.RandomAccess).

Secrets are read ONLY from:
  * a secure interactive prompt (Read-Host -AsSecureString), the default; or
  * standard input, when -FromStdin is given and stdin is redirected. Exactly one leaf
    per stdin run. The bytes are sealed exactly as received, never trimmed or mutated.
    A DNS token with a trailing CR/LF is refused (arxd refuses it too); write the token
    without a newline. Tunnel JSON may carry JSON whitespace (space, tab, CR, LF),
    including a trailing newline, because arxd's serde_json parse accepts it.

There are deliberately no parameters or environment variables for secret values. The
script refuses to run while a PowerShell transcript is active, under an elevated token
(native TokenElevation / TokenElevationType, refused when unreadable), or when stdin is
redirected without -FromStdin. Plaintext stays inside the compiled helper below, in
pinned byte arrays only (never a managed string or a PowerShell variable); it is never
printed, and every plaintext buffer, including the full-length native copy of an
interactive SecureString, is zeroed in `finally`. Only a receipt with output paths,
ciphertext lengths and ciphertext SHA-256 digests is written to stdout. Refusals print a
fixed reason code on stderr (exit 2).

Validation happens before anything is written:
  * DNS, identical to cloudflare.rs DnsBearer::new after runtime.rs native_dns's
    512-byte read limit: 1..=512 bytes, each in [A-Za-z0-9-_.~+/=].
  * Tunnel, a strict SUBSET of what connector.rs TunnelCredential::load accepts (read
    limit 2048), so every sealed file is accepted by arxd. The sealer accepts only a
    canonical JSON object: optional JSON whitespace, unescaped case-sensitive keys
    AccountTag, TunnelSecret, TunnelID and optional Endpoint, no unknown or duplicate
    keys, string values only, printable ASCII without escapes inside strings. It
    deliberately refuses inputs serde_json would also take but cloudflared never
    writes: the JSON-array (sequence) form of the struct and escaped key spellings such
    as "Account\u0054ag". Values then follow arxd exactly: AccountTag equals -AccountId
    (32 lowercase hex), TunnelID equals -TunnelId (TunnelId::parse: 36 bytes, '-' at
    8/13/18/23, lowercase hex elsewhere), Endpoint is empty when present, and
    TunnelSecret is canonical standard base64 of 32 bytes. All checks run over bytes.

Custody follows publish/windows.rs read_secret and create_private:
  * The target directory must lie strictly under D:\ArxcovoData and pass arxd's
    LocalPath rules (drive-letter path, <= 240 bytes, safe components only, so no '..',
    ADS ':' or '~' short names). D:\ must be a fixed drive.
  * Every component below D:\ is opened with NtCreateFile relative to the already held
    parent handle (OBJECT_ATTRIBUTES.RootDirectory, FILE_OPEN_REPARSE_POINT), never by
    re-resolving an absolute path, and must be a directory without a reparse point. All
    handles stay open without delete sharing until the run ends. With -CreateDirectory
    the final component is opened-or-created in that same relative call (FILE_OPEN_IF)
    with the private descriptor, and that exact handle is retained and checked.
    GetFinalPathNameByHandle must return the requested spelling.
  * The target directory must be owned by the current user and carry a protected DACL
    (inheritance disabled) whose entries are basic ACCESS_ALLOWED ACEs for the current
    user only (callback, object, deny and inherited entries are refused).
  * Each leaf is created with NtCreateFile FILE_CREATE relative to the held target
    handle (an existing file, hard link or reparse point is refused, never replaced),
    no sharing, and owner O:<user> with protected DACL D:P(A;;FA;;;<user>) set in the
    creation call. CryptProtectData runs with no entropy, no description and
    CRYPTPROTECT_UI_FORBIDDEN in user scope, matching arxd's CryptUnprotectData call.
  * On that same creation handle the sealer checks a plain file with one link, owner and
    private DACL, writes and flushes, reads back <= 16384 bytes, compares, checks the
    user-scope DPAPI header, decrypts within arxd's read limit, compares with the
    validated plaintext and re-validates it. Any failure deletes the leaf through the
    same handle, as publish/windows.rs delete_on_close does (FileDispositionInfoEx with
    DELETE | IGNORE_READONLY_ATTRIBUTE, falling back to the classic
    disposition); a cleanup that cannot be confirmed is reported as residue.

-TestFault exists only for the test suite: it can only force a refusal after the leaf
was created (a corrupted read-back, optionally after marking the leaf read-only), so the
same-handle cleanup is exercised. It can never make an input or custody check pass.

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

    [switch]$CreateDirectory,

    [Parameter(DontShow = $true)]
    [ValidateSet('None', 'CorruptReadback', 'ReadOnlyCorruptReadback')]
    [string]$TestFault = 'None'
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
        public const uint SYNCHRONIZE = 0x00100000;
        public const uint FILE_LIST_DIRECTORY = 0x1;
        public const uint FILE_TRAVERSE = 0x20;
        public const uint FILE_READ_ATTRIBUTES = 0x80;
        public const uint FILE_SHARE_READ = 1;
        public const uint FILE_SHARE_WRITE = 2;
        public const uint FILE_SHARE_DELETE = 4;
        public const uint OPEN_EXISTING = 3;
        public const uint FILE_ATTRIBUTE_READONLY = 0x1;
        public const uint FILE_ATTRIBUTE_DIRECTORY = 0x10;
        public const uint FILE_ATTRIBUTE_NORMAL = 0x80;
        public const uint FILE_ATTRIBUTE_REPARSE_POINT = 0x400;
        public const uint FILE_FLAG_BACKUP_SEMANTICS = 0x02000000;
        public const uint FILE_FLAG_OPEN_REPARSE_POINT = 0x00200000;
        public const uint OWNER_SECURITY_INFORMATION = 1;
        public const uint DACL_SECURITY_INFORMATION = 4;
        public const uint CRYPTPROTECT_UI_FORBIDDEN = 1;
        public const uint TOKEN_QUERY = 8;
        public const int TokenElevationType = 18;
        public const int TokenElevation = 20;
        public const int ERROR_INVALID_PARAMETER = 87;

        // NtCreateFile
        public const uint FILE_OPEN = 1;
        public const uint FILE_CREATE = 2;
        public const uint FILE_OPEN_IF = 3;
        public const uint FILE_DIRECTORY_FILE = 0x1;
        public const uint FILE_SYNCHRONOUS_IO_NONALERT = 0x20;
        public const uint FILE_NON_DIRECTORY_FILE = 0x40;
        public const uint FILE_OPEN_REPARSE_POINT = 0x00200000;
        public const uint OBJ_CASE_INSENSITIVE = 0x40;
        public const int STATUS_OBJECT_NAME_NOT_FOUND = unchecked((int)0xC0000034);
        public const int STATUS_OBJECT_NAME_COLLISION = unchecked((int)0xC0000035);
        public const int STATUS_OBJECT_PATH_NOT_FOUND = unchecked((int)0xC000003A);

        // SetFileInformationByHandle classes and disposition flags (delete_on_close).
        public const int FileBasicInfo = 0;
        public const int FileDispositionInfo = 4;
        public const int FileDispositionInfoEx = 21;
        public const uint FILE_DISPOSITION_FLAG_DELETE = 0x1;
        public const uint FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE = 0x10;

        [StructLayout(LayoutKind.Sequential)]
        public struct UNICODE_STRING
        {
            public ushort Length;
            public ushort MaximumLength;
            public IntPtr Buffer;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct OBJECT_ATTRIBUTES
        {
            public int Length;
            public IntPtr RootDirectory;
            public IntPtr ObjectName;
            public uint Attributes;
            public IntPtr SecurityDescriptor;
            public IntPtr SecurityQualityOfService;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct IO_STATUS_BLOCK
        {
            public IntPtr Status;
            public IntPtr Information;
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

        [StructLayout(LayoutKind.Sequential)]
        public struct FILE_DISPOSITION_INFO_EX
        {
            public uint Flags;
        }

        [StructLayout(LayoutKind.Sequential)]
        public struct FILE_BASIC_INFO
        {
            public long CreationTime;
            public long LastAccessTime;
            public long LastWriteTime;
            public long ChangeTime;
            public uint FileAttributes;
        }

        [DllImport("ntdll.dll")]
        public static extern int NtCreateFile(out IntPtr handle, uint access, ref OBJECT_ATTRIBUTES attributes,
            out IO_STATUS_BLOCK status, IntPtr allocationSize, uint fileAttributes, uint share,
            uint disposition, uint options, IntPtr eaBuffer, uint eaLength);

        [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
        public static extern SafeFileHandle CreateFileW(string name, uint access, uint share,
            IntPtr security, uint disposition, uint flags, IntPtr template);

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

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetFileInformationByHandle(SafeFileHandle file, int infoClass,
            ref FILE_DISPOSITION_INFO_EX info, uint size);

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool SetFileInformationByHandle(SafeFileHandle file, int infoClass,
            ref FILE_BASIC_INFO info, uint size);

        [DllImport("kernel32.dll")]
        public static extern IntPtr GetCurrentProcess();

        [DllImport("kernel32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool CloseHandle(IntPtr handle);

        [DllImport("kernel32.dll")]
        public static extern IntPtr LocalFree(IntPtr memory);

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool OpenProcessToken(IntPtr process, uint access, out IntPtr token);

        [DllImport("advapi32.dll", SetLastError = true)]
        [return: MarshalAs(UnmanagedType.Bool)]
        public static extern bool GetTokenInformation(IntPtr token, int infoClass, out int information,
            uint length, out uint returned);

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

    /// One validated plaintext leaf. The pinned bytes never leave this object and are zeroed on Dispose.
    public sealed class LeafPlan : IDisposable
    {
        internal readonly string Kind;
        internal readonly string LeafName;
        internal readonly int Limit;
        internal readonly byte[] Account;
        internal readonly byte[] Tunnel;
        internal byte[] Plaintext;

        internal LeafPlan(string kind, byte[] plaintext, byte[] account, byte[] tunnel)
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

    /// The fenced target directory and its whole retained, handle-bound ancestor chain.
    public sealed class Target : IDisposable
    {
        internal readonly string Directory;
        internal readonly SafeFileHandle[] Chain;
        internal Target(string directory, SafeFileHandle[] chain) { Directory = directory; Chain = chain; }
        internal SafeFileHandle Handle { get { return Chain[Chain.Length - 1]; } }
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
        const uint DirectoryAccess = Native.FILE_LIST_DIRECTORY | Native.FILE_TRAVERSE
            | Native.FILE_READ_ATTRIBUTES | Native.READ_CONTROL | Native.SYNCHRONIZE;

        static readonly byte[] KeyAccountTag = Encoding.ASCII.GetBytes("AccountTag");
        static readonly byte[] KeyTunnelSecret = Encoding.ASCII.GetBytes("TunnelSecret");
        static readonly byte[] KeyTunnelId = Encoding.ASCII.GetBytes("TunnelID");
        static readonly byte[] KeyEndpoint = Encoding.ASCII.GetBytes("Endpoint");
        static readonly byte[] SecretLastSextet = Encoding.ASCII.GetBytes("AEIMQUYcgkosw048");

        static SealRefusedException Refuse(string code) { return new SealRefusedException(code); }

        /// Plaintext buffers are pinned so the GC never leaves unzeroed copies behind.
        static byte[] Pinned(int length) { return GC.AllocateArray<byte>(length, true); }

        static SecurityIdentifier User()
        {
            using (WindowsIdentity identity = WindowsIdentity.GetCurrent()) { return identity.User; }
        }

        // ---- token ---------------------------------------------------------------------------

        /// vm/windows.rs: TokenElevation must be 0 and TokenElevationType not Full; unreadable refuses.
        public static string ElevationRefusal()
        {
            IntPtr token;
            if (!Native.OpenProcessToken(Native.GetCurrentProcess(), Native.TOKEN_QUERY, out token))
                return "elevation-unknown";
            try
            {
                int elevated, kind;
                uint returned;
                if (!Native.GetTokenInformation(token, Native.TokenElevation, out elevated, 4, out returned) || returned != 4)
                    return "elevation-unknown";
                if (!Native.GetTokenInformation(token, Native.TokenElevationType, out kind, 4, out returned) || returned != 4)
                    return "elevation-unknown";
                if (elevated != 0 || kind == 2) return "elevated-token";
                if (kind != 1 && kind != 3) return "elevation-unknown";
                return null;
            }
            finally { Native.CloseHandle(token); }
        }

        // ---- identifiers (publish/mod.rs hex_id, TunnelId::parse) -----------------------------

        static bool LowerHex(byte c) { return (c >= (byte)'0' && c <= (byte)'9') || (c >= (byte)'a' && c <= (byte)'f'); }

        static bool HexId(byte[] v, int start, int length)
        {
            if (length != 32) return false;
            for (int i = 0; i < 32; i++) if (!LowerHex(v[start + i])) return false;
            return true;
        }

        static bool TunnelIdValid(byte[] v, int start, int length)
        {
            if (length != 36) return false;
            for (int i = 0; i < 36; i++)
            {
                byte c = v[start + i];
                if (i == 8 || i == 13 || i == 18 || i == 23) { if (c != (byte)'-') return false; }
                else if (!LowerHex(c)) return false;
            }
            return true;
        }

        static byte[] AsciiArgument(string value)
        {
            if (value == null) return new byte[0];
            foreach (char c in value) if (c > 0x7E || c < 0x20) return new byte[0];
            return Encoding.ASCII.GetBytes(value);
        }

        static bool SliceEquals(byte[] v, int start, int length, byte[] expected)
        {
            if (length != expected.Length) return false;
            for (int i = 0; i < length; i++) if (v[start + i] != expected[i]) return false;
            return true;
        }

        // ---- payload rules (bytes only; no managed strings over payload data) ------------------

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

        static void SkipSpace(byte[] v, ref int i) { while (i < v.Length && JsonSpace(v[i])) i++; }

        /// One unescaped printable-ASCII JSON string, returned as a byte range.
        static string ReadString(byte[] v, ref int i, out int start, out int length)
        {
            start = 0;
            length = 0;
            if (i >= v.Length || v[i] != (byte)'"') return "tunnel-json-non-string-value";
            i++;
            start = i;
            while (i < v.Length && v[i] != (byte)'"')
            {
                byte b = v[i];
                if (b == (byte)'\\') return "tunnel-json-escape";
                if (b < 0x20 || b > 0x7E) return "tunnel-json-forbidden-byte";
                i++;
            }
            if (i >= v.Length) return "tunnel-json-malformed";
            length = i - start;
            i++;
            return null;
        }

        /// The strict canonical-object subset of connector.rs CredentialJson + TunnelCredential::load.
        public static string CheckTunnel(byte[] v, byte[] account, byte[] tunnel)
        {
            if (v.Length == 0) return "tunnel-json-empty";
            if (v.Length > TunnelLimit) return "tunnel-json-too-long";
            if (v.Length >= 3 && v[0] == 0xEF && v[1] == 0xBB && v[2] == 0xBF) return "tunnel-json-bom";
            int accountStart = -1, accountLength = 0, secretStart = -1, secretLength = 0;
            int tunnelStart = -1, tunnelLength = 0, endpointStart = -1, endpointLength = 0;
            int i = 0;
            SkipSpace(v, ref i);
            if (i >= v.Length || v[i] != (byte)'{') return "tunnel-json-not-object";
            i++;
            SkipSpace(v, ref i);
            if (i < v.Length && v[i] == (byte)'}') return "tunnel-json-missing-field";
            while (true)
            {
                int keyStart, keyLength, valueStart, valueLength;
                if (i >= v.Length || v[i] != (byte)'"') return "tunnel-json-malformed";
                string error = ReadString(v, ref i, out keyStart, out keyLength);
                if (error != null) return error;
                SkipSpace(v, ref i);
                if (i >= v.Length || v[i] != (byte)':') return "tunnel-json-malformed";
                i++;
                SkipSpace(v, ref i);
                error = ReadString(v, ref i, out valueStart, out valueLength);
                if (error != null) return error;
                if (SliceEquals(v, keyStart, keyLength, KeyAccountTag))
                {
                    if (accountStart >= 0) return "tunnel-json-duplicate-field";
                    accountStart = valueStart; accountLength = valueLength;
                }
                else if (SliceEquals(v, keyStart, keyLength, KeyTunnelSecret))
                {
                    if (secretStart >= 0) return "tunnel-json-duplicate-field";
                    secretStart = valueStart; secretLength = valueLength;
                }
                else if (SliceEquals(v, keyStart, keyLength, KeyTunnelId))
                {
                    if (tunnelStart >= 0) return "tunnel-json-duplicate-field";
                    tunnelStart = valueStart; tunnelLength = valueLength;
                }
                else if (SliceEquals(v, keyStart, keyLength, KeyEndpoint))
                {
                    if (endpointStart >= 0) return "tunnel-json-duplicate-field";
                    endpointStart = valueStart; endpointLength = valueLength;
                }
                else return "tunnel-json-unknown-field";
                SkipSpace(v, ref i);
                if (i < v.Length && v[i] == (byte)',') { i++; SkipSpace(v, ref i); continue; }
                if (i < v.Length && v[i] == (byte)'}') { i++; break; }
                return "tunnel-json-malformed";
            }
            SkipSpace(v, ref i);
            if (i != v.Length) return "tunnel-json-trailing-data";
            if (accountStart < 0 || secretStart < 0 || tunnelStart < 0) return "tunnel-json-missing-field";
            if (!HexId(v, accountStart, accountLength)) return "tunnel-account-tag-invalid";
            if (!SliceEquals(v, accountStart, accountLength, account)) return "tunnel-account-tag-mismatch";
            if (!TunnelIdValid(v, tunnelStart, tunnelLength)) return "tunnel-id-invalid";
            if (!SliceEquals(v, tunnelStart, tunnelLength, tunnel)) return "tunnel-id-mismatch";
            if (endpointStart >= 0 && endpointLength != 0) return "tunnel-endpoint-not-empty";
            if (secretLength != 44 || v[secretStart + 43] != (byte)'=') return "tunnel-secret-not-canonical";
            for (int k = 0; k < 43; k++)
            {
                byte c = v[secretStart + k];
                bool ok = (c >= (byte)'A' && c <= (byte)'Z') || (c >= (byte)'a' && c <= (byte)'z')
                    || (c >= (byte)'0' && c <= (byte)'9') || c == (byte)'+' || c == (byte)'/';
                if (!ok) return "tunnel-secret-not-canonical";
            }
            if (Array.IndexOf(SecretLastSextet, v[secretStart + 42]) < 0) return "tunnel-secret-not-canonical";
            return null;
        }

        static string CheckPayload(string kind, byte[] plaintext, byte[] account, byte[] tunnel)
        {
            return kind == "dns" ? CheckDns(plaintext) : CheckTunnel(plaintext, account, tunnel);
        }

        static void Identifiers(string kind, string account, string tunnel, out byte[] accountBytes, out byte[] tunnelBytes)
        {
            accountBytes = new byte[0];
            tunnelBytes = new byte[0];
            if (kind != "tunnel") return;
            accountBytes = AsciiArgument(account);
            tunnelBytes = AsciiArgument(tunnel);
            if (!HexId(accountBytes, 0, accountBytes.Length)) throw Refuse("account-id-invalid");
            if (!TunnelIdValid(tunnelBytes, 0, tunnelBytes.Length)) throw Refuse("tunnel-id-argument-invalid");
        }

        static LeafPlan Validated(string kind, byte[] plaintext, byte[] account, byte[] tunnel)
        {
            string error = CheckPayload(kind, plaintext, account, tunnel);
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
            byte[] accountBytes, tunnelBytes;
            Identifiers(kind, account, tunnel, out accountBytes, out tunnelBytes);
            if (!Console.IsInputRedirected) throw Refuse("stdin-not-redirected");
            int limit = kind == "dns" ? DnsLimit : TunnelLimit;
            byte[] buffer = Pinned(limit + 1);
            byte[] exact = null;
            bool handed = false;
            try
            {
                int total = 0;
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
                exact = Pinned(total);
                Buffer.BlockCopy(buffer, 0, exact, 0, total);
                LeafPlan plan = Validated(kind, exact, accountBytes, tunnelBytes);
                handed = true;
                return plan;
            }
            finally
            {
                Array.Clear(buffer, 0, buffer.Length);
                if (!handed && exact != null) Array.Clear(exact, 0, exact.Length);
            }
        }

        /// Converts one interactive SecureString entry (UTF-16) to its exact ASCII bytes. The native
        /// copy is wiped over its full known length (not up to a NUL) before it is freed.
        public static LeafPlan FromSecureString(string kindName, SecureString value, string account, string tunnel)
        {
            string kind = NormalizeKind(kindName);
            byte[] accountBytes, tunnelBytes;
            Identifiers(kind, account, tunnel, out accountBytes, out tunnelBytes);
            int limit = kind == "dns" ? DnsLimit : TunnelLimit;
            if (value == null || value.Length == 0) throw Refuse(kind == "dns" ? "dns-token-empty" : "tunnel-json-empty");
            int length = value.Length;
            if (length > limit) throw Refuse(kind == "dns" ? "dns-token-too-long" : "tunnel-json-too-long");
            byte[] bytes = Pinned(length);
            bool handed = false;
            IntPtr raw = IntPtr.Zero;
            try
            {
                raw = Marshal.SecureStringToGlobalAllocUnicode(value);
                for (int i = 0; i < length; i++)
                {
                    int c = (ushort)Marshal.ReadInt16(raw, 2 * i);
                    if (c > 0x7F) throw Refuse(kind == "dns" ? "dns-token-forbidden-byte" : "tunnel-json-forbidden-byte");
                    bytes[i] = (byte)c;
                }
                LeafPlan plan = Validated(kind, bytes, accountBytes, tunnelBytes);
                handed = true;
                return plan;
            }
            finally
            {
                if (raw != IntPtr.Zero)
                {
                    // length UTF-16 units plus the terminator SecureStringToGlobalAllocUnicode appends.
                    for (int i = 0; i < (length + 1) * 2; i++) Marshal.WriteByte(raw, i, 0);
                    Marshal.FreeHGlobal(raw);
                }
                if (!handed) Array.Clear(bytes, 0, bytes.Length);
            }
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
            if (stem.Length == 4 && (stem.StartsWith("COM") || stem.StartsWith("LPT")) && stem[3] >= '0'
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

        /// NtCreateFile of one plain component relative to a held directory handle.
        static int OpenRelative(SafeFileHandle parent, string name, uint access, uint share, uint disposition,
            uint options, uint attributes, IntPtr descriptor, out SafeFileHandle handle, out long information)
        {
            handle = null;
            information = 0;
            IntPtr buffer = Marshal.StringToHGlobalUni(name);
            IntPtr unicode = Marshal.AllocHGlobal(Marshal.SizeOf(typeof(Native.UNICODE_STRING)));
            bool added = false;
            try
            {
                parent.DangerousAddRef(ref added);
                Native.UNICODE_STRING text = new Native.UNICODE_STRING();
                text.Length = (ushort)(name.Length * 2);
                text.MaximumLength = (ushort)(name.Length * 2);
                text.Buffer = buffer;
                Marshal.StructureToPtr(text, unicode, false);
                Native.OBJECT_ATTRIBUTES objectAttributes = new Native.OBJECT_ATTRIBUTES();
                objectAttributes.Length = Marshal.SizeOf(typeof(Native.OBJECT_ATTRIBUTES));
                objectAttributes.RootDirectory = parent.DangerousGetHandle();
                objectAttributes.ObjectName = unicode;
                objectAttributes.Attributes = Native.OBJ_CASE_INSENSITIVE;
                objectAttributes.SecurityDescriptor = descriptor;
                IntPtr raw;
                Native.IO_STATUS_BLOCK io;
                int status = Native.NtCreateFile(out raw, access, ref objectAttributes, out io, IntPtr.Zero,
                    attributes, share, disposition, options, IntPtr.Zero, 0);
                if (status >= 0)
                {
                    handle = new SafeFileHandle(raw, true);
                    information = io.Information.ToInt64();
                }
                return status;
            }
            finally
            {
                if (added) parent.DangerousRelease();
                Marshal.FreeHGlobal(unicode);
                Marshal.FreeHGlobal(buffer);
            }
        }

        static void RequirePlainDirectory(SafeFileHandle handle)
        {
            Native.BY_HANDLE_FILE_INFORMATION info;
            if (!Native.GetFileInformationByHandle(handle, out info)) throw Refuse("ancestor-unreadable");
            if ((info.dwFileAttributes & Native.FILE_ATTRIBUTE_REPARSE_POINT) != 0) throw Refuse("reparse-point-in-path");
            if ((info.dwFileAttributes & Native.FILE_ATTRIBUTE_DIRECTORY) == 0) throw Refuse("ancestor-not-directory");
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

        /// Owner is the current user; protected DACL; only basic, explicit ACCESS_ALLOWED entries
        /// for the current user (security.rs private accepts basic allow/deny types only, so
        /// callback and object entries are refused here as well).
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
                GenericAce entry = acl[i];
                if (entry.AceType != AceType.AccessAllowed) return false;
                CommonAce ace = entry as CommonAce;
                if (ace == null || ace.IsCallback || ace.AceQualifier != AceQualifier.AccessAllowed) return false;
                if (!ace.SecurityIdentifier.Equals(user)) return false;
                if ((ace.AceFlags & AceFlags.Inherited) != 0) return false;
            }
            return true;
        }

        /// publish/windows.rs create_private: owner and a protected DACL granting only TokenUser.
        /// A created directory's single entry is also inheritable (OICI) so later children stay private.
        static IntPtr PrivateDescriptor(bool directory)
        {
            string sid = User().Value;
            string sddl = "O:" + sid + "D:P(A;" + (directory ? "OICI" : "") + ";FA;;;" + sid + ")";
            IntPtr descriptor;
            if (!Native.ConvertStringSecurityDescriptorToSecurityDescriptorW(sddl, 1, out descriptor, IntPtr.Zero))
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

        /// Fences, walks handle-relative from D:\, optionally creates the final component in the
        /// same relative call, and retains the whole chain.
        public static Target Prepare(string rawDirectory, bool create)
        {
            string directory = FencePath(rawDirectory);
            if (Native.GetDriveTypeW("D:\\") != 3) throw Refuse("drive-not-fixed");
            string[] parts = directory.Substring(3).Split('\\');
            SafeFileHandle[] chain = new SafeFileHandle[parts.Length + 1];
            bool done = false;
            try
            {
                chain[0] = Native.CreateFileW("D:\\", DirectoryAccess, Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE,
                    IntPtr.Zero, Native.OPEN_EXISTING,
                    Native.FILE_FLAG_BACKUP_SEMANTICS | Native.FILE_FLAG_OPEN_REPARSE_POINT, IntPtr.Zero);
                if (chain[0].IsInvalid) throw Refuse("ancestor-unopenable");
                RequirePlainDirectory(chain[0]);
                for (int i = 0; i < parts.Length; i++)
                {
                    bool last = i == parts.Length - 1;
                    bool creating = last && create;
                    IntPtr descriptor = creating ? PrivateDescriptor(true) : IntPtr.Zero;
                    int status;
                    SafeFileHandle handle;
                    long information;
                    try
                    {
                        status = OpenRelative(chain[i], parts[i], DirectoryAccess,
                            Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE,
                            creating ? Native.FILE_OPEN_IF : Native.FILE_OPEN,
                            Native.FILE_DIRECTORY_FILE | Native.FILE_OPEN_REPARSE_POINT | Native.FILE_SYNCHRONOUS_IO_NONALERT,
                            creating ? Native.FILE_ATTRIBUTE_NORMAL : 0, descriptor, out handle, out information);
                    }
                    finally { if (descriptor != IntPtr.Zero) Native.LocalFree(descriptor); }
                    if (status < 0)
                    {
                        if (status == Native.STATUS_OBJECT_NAME_NOT_FOUND || status == Native.STATUS_OBJECT_PATH_NOT_FOUND)
                            throw Refuse(last ? "target-directory-missing" : "ancestor-missing");
                        throw Refuse(creating ? "target-directory-create-failed" : "ancestor-unopenable");
                    }
                    chain[i + 1] = handle;
                    RequirePlainDirectory(handle);
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

        /// Early refusal before any secret is read; FILE_CREATE in Seal stays authoritative.
        public static void EnsureAbsent(Target target, string kindName)
        {
            string leaf = NormalizeKind(kindName) == "dns" ? "dns.dpapi" : "tunnel.dpapi";
            SafeFileHandle handle;
            long information;
            int status = OpenRelative(target.Handle, leaf, Native.FILE_READ_ATTRIBUTES | Native.SYNCHRONIZE,
                Native.FILE_SHARE_READ | Native.FILE_SHARE_WRITE | Native.FILE_SHARE_DELETE, Native.FILE_OPEN,
                Native.FILE_OPEN_REPARSE_POINT | Native.FILE_SYNCHRONOUS_IO_NONALERT, 0, IntPtr.Zero,
                out handle, out information);
            if (status >= 0)
            {
                handle.Dispose();
                throw Refuse("target-file-exists");
            }
            if (status != Native.STATUS_OBJECT_NAME_NOT_FOUND) throw Refuse("target-file-unknown");
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

        /// Returns a pinned plaintext copy within the read limit; the DPAPI allocation is zeroed.
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
                    byte[] result = Pinned(output.cbData);
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

        static bool Same(byte[] a, byte[] b) { return CryptographicOperations.FixedTimeEquals(a, b); }

        /// Mirrors publish/windows.rs delete_on_close: marks the file this run created for deletion
        /// through its own handle with the extended disposition, which ignores a read-only attribute.
        /// Only where the extended class is reported unsupported (ERROR_INVALID_PARAMETER) is the
        /// classic disposition tried on the same handle. Failure is returned, never retried by path.
        static bool DeleteOnClose(SafeFileHandle handle)
        {
            Native.FILE_DISPOSITION_INFO_EX extended = new Native.FILE_DISPOSITION_INFO_EX();
            extended.Flags = Native.FILE_DISPOSITION_FLAG_DELETE | Native.FILE_DISPOSITION_FLAG_IGNORE_READONLY_ATTRIBUTE;
            if (Native.SetFileInformationByHandle(handle, Native.FileDispositionInfoEx, ref extended, 4)) return true;
            if (Marshal.GetLastWin32Error() != Native.ERROR_INVALID_PARAMETER) return false;
            Native.FILE_DISPOSITION_INFO classic = new Native.FILE_DISPOSITION_INFO();
            classic.DeleteFile = 1;
            return Native.SetFileInformationByHandle(handle, Native.FileDispositionInfo, ref classic, 1);
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
        static byte[] ReadAll(SafeFileHandle handle)
        {
            byte[] buffer = new byte[CiphertextLimit + 1];
            int total = 0, read;
            while (total < buffer.Length
                && (read = RandomAccess.Read(handle, new Span<byte>(buffer, total, buffer.Length - total), total)) > 0)
                total += read;
            if (total > CiphertextLimit) throw Refuse("readback-too-large");
            byte[] exact = new byte[total];
            Buffer.BlockCopy(buffer, 0, exact, 0, total);
            return exact;
        }

        static void MarkReadOnly(SafeFileHandle handle)
        {
            Native.FILE_BASIC_INFO info = new Native.FILE_BASIC_INFO();
            info.FileAttributes = Native.FILE_ATTRIBUTE_READONLY;
            if (!Native.SetFileInformationByHandle(handle, Native.FileBasicInfo, ref info,
                    (uint)Marshal.SizeOf(typeof(Native.FILE_BASIC_INFO))))
                throw Refuse("test-fault-readonly-failed");
        }

        // ---- sealing ---------------------------------------------------------------------------

        public static LeafReceipt Seal(Target target, LeafPlan plan, string fault)
        {
            if (plan == null || plan.Plaintext == null) throw Refuse("plan-consumed");
            if (fault != "None" && fault != "CorruptReadback" && fault != "ReadOnlyCorruptReadback")
                throw Refuse("test-fault-invalid");
            string path = target.Directory + "\\" + plan.LeafName;
            byte[] ciphertext = Protect(plan.Plaintext);
            if (ciphertext.Length > CiphertextLimit || !UserScopeDpapi(ciphertext)) throw Refuse("dpapi-blob-unsupported");
            IntPtr descriptor = PrivateDescriptor(false);
            int status;
            SafeFileHandle file;
            long information;
            try
            {
                // FILE_CREATE relative to the held directory never opens, follows or replaces an existing name.
                status = OpenRelative(target.Handle, plan.LeafName,
                    Native.GENERIC_READ | Native.GENERIC_WRITE | Native.DELETE | Native.SYNCHRONIZE, 0,
                    Native.FILE_CREATE,
                    Native.FILE_NON_DIRECTORY_FILE | Native.FILE_OPEN_REPARSE_POINT | Native.FILE_SYNCHRONOUS_IO_NONALERT,
                    Native.FILE_ATTRIBUTE_NORMAL, descriptor, out file, out information);
            }
            finally { Native.LocalFree(descriptor); }
            if (status == Native.STATUS_OBJECT_NAME_COLLISION) throw Refuse("target-file-exists");
            if (status < 0) throw Refuse("target-file-create-failed");
            using (file)
            {
                try
                {
                    CheckLeafHandle(file);
                    if (!OwnerPrivate(file)) throw Refuse("leaf-not-private");
                    RandomAccess.Write(file, new ReadOnlySpan<byte>(ciphertext), 0);
                    if (!Native.FlushFileBuffers(file)) throw Refuse("leaf-flush-failed");
                    if (fault == "ReadOnlyCorruptReadback") MarkReadOnly(file);
                    // read_secret's checks, on the exact created leaf and held directory handles.
                    CheckLeafHandle(file);
                    if (!OwnerPrivate(file)) throw Refuse("leaf-not-private");
                    if (!OwnerPrivate(target.Handle)) throw Refuse("target-directory-not-private");
                    byte[] back = ReadAll(file);
                    if (fault != "None" && back.Length > 0) back[back.Length - 1] ^= 0xFF;
                    if (!Same(back, ciphertext) || !UserScopeDpapi(back)) throw Refuse("readback-mismatch");
                    byte[] plain = Unprotect(back, plan.Limit);
                    try
                    {
                        if (!Same(plain, plan.Plaintext)) throw Refuse("readback-plaintext-mismatch");
                        string error = CheckPayload(plan.Kind, plain, plan.Account, plan.Tunnel);
                        if (error != null) throw Refuse("readback-" + error);
                    }
                    finally { Array.Clear(plain, 0, plain.Length); }
                }
                catch (SealRefusedException refused)
                {
                    if (!DeleteOnClose(file)) throw Refuse(refused.Code + "-residue-unremoved");
                    throw;
                }
                catch
                {
                    if (!DeleteOnClose(file)) throw Refuse("leaf-residue-unremoved");
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

if (-not ('Arxcovo.CloudflareSeal.Sealer' -as [type])) {
    Add-Type -TypeDefinition $source -Language CSharp
}

$elevation = [Arxcovo.CloudflareSeal.Sealer]::ElevationRefusal()
if ($null -ne $elevation) { Write-Refusal $elevation }

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
            $receipt = [Arxcovo.CloudflareSeal.Sealer]::Seal($target, $plan, $TestFault)
            [ordered]@{
                kind              = $receipt.Kind
                path              = $receipt.Path
                ciphertext_bytes  = $receipt.CiphertextBytes
                ciphertext_sha256 = $receipt.CiphertextSha256
            }
        }
        [ordered]@{
            schema                 = 'arxcovo.cloudflare-seal.v1'
            target_directory       = $target.Path
            dpapi_scope            = 'current_user'
            leaves                 = @($leaves)
            daemon_config_modified = $false
            public_publishing      = 'unchanged-default-off'
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
