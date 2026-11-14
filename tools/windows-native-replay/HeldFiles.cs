// SOURCE ONLY. No public entry point; compiling this file invokes no native API.
// Used only by the dormant manifest adapter. No copy or hardlink fallback.
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;

internal sealed class HeldFiles : IDisposable {
    [StructLayout(LayoutKind.Sequential)]
    struct Info {
        internal uint attributes, creationLow, creationHigh, accessLow, accessHigh,
            writeLow, writeHigh, volume, sizeHigh, sizeLow, links, idHigh, idLow;
    }
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true, ExactSpelling=true)]
    static extern SafeFileHandle CreateFileW(string path,uint access,uint share,IntPtr security,uint disposition,uint flags,IntPtr template);
    [DllImport("kernel32.dll", SetLastError=true, ExactSpelling=true)]
    static extern bool GetFileInformationByHandle(SafeFileHandle file,out Info info);
    [DllImport("kernel32.dll", SetLastError=true, ExactSpelling=true)]
    static extern uint GetFileType(SafeFileHandle file);
    readonly List<SafeFileHandle> ancestors=new List<SafeFileHandle>();
    readonly List<FileStream> files=new List<FileStream>();
    readonly uint volume;
    readonly string root;
    bool disposed;
    internal HeldFiles(uint approvedVolume,string approvedRoot) {
        if(approvedVolume==0 || !ReplayController.LaunchSpec.Absolute(approvedRoot)) throw new ArgumentException("root");
        volume=approvedVolume; root=approvedRoot;
    }
    static IOException Refused() { return new IOException("file pin refused"); }
    internal static bool MetadataValid(uint attributes,uint type,uint links,long size,long limit) {
        return (attributes & (0x400U|0x10U))==0 && type==1 && links==1 && size>=0 && size<=limit && limit>0 && limit<=67108864;
    }
    internal static bool HashMatches(byte[] bytes,string externalExpectedHash,int limit) {
        if(bytes==null || limit<=0 || limit>67108864 || bytes.Length>limit || externalExpectedHash==null || externalExpectedHash.Length!=64) return false;
        foreach(char c in externalExpectedHash) if(!((c>='0' && c<='9') || (c>='a' && c<='f'))) return false;
        using(var sha=SHA256.Create()) {
            string actual=BitConverter.ToString(sha.ComputeHash(bytes)).Replace("-","").ToLowerInvariant();
            return actual==externalExpectedHash;
        }
    }
    [StructLayout(LayoutKind.Sequential)]
    struct UnicodeName { internal ushort length, maximum; internal IntPtr buffer; }
    [StructLayout(LayoutKind.Sequential)]
    struct Attributes { internal int length; internal IntPtr parent, name; internal uint flags; internal IntPtr security, qos; }
    [StructLayout(LayoutKind.Sequential)]
    struct IoStatus { internal IntPtr status; internal UIntPtr information; }
    [StructLayout(LayoutKind.Sequential)]
    struct FileId { internal ulong volume, low, high; }
    [DllImport("ntdll.dll", ExactSpelling=true)]
    static extern int NtCreateFile(out SafeFileHandle handle,uint access,ref Attributes attributes,
        out IoStatus status,IntPtr allocation,uint fileAttributes,uint share,uint disposition,
        uint options,IntPtr ea,uint eaLength);
    [DllImport("kernel32.dll", SetLastError=true, ExactSpelling=true)]
    static extern bool GetFileInformationByHandleEx(SafeFileHandle file,int kind,out FileId id,uint size);
    static SafeFileHandle Child(SafeFileHandle parent,string name,bool directory) {
        IntPtr text=Marshal.StringToHGlobalUni(name), descriptor=IntPtr.Zero;
        try {
            var u=new UnicodeName {length=checked((ushort)(name.Length*2)),maximum=checked((ushort)(name.Length*2)),buffer=text};
            descriptor=Marshal.AllocHGlobal(Marshal.SizeOf<UnicodeName>());
            Marshal.StructureToPtr(u,descriptor,false);
            var a=new Attributes {length=Marshal.SizeOf<Attributes>(),parent=parent.DangerousGetHandle(),name=descriptor,flags=0x40};
            IoStatus io; SafeFileHandle child;
            // OBJ relative, FILE_OPEN, synchronous nonalert, OPEN_REPARSE_POINT;
            // no name-based reopening and no write/delete sharing.
            int status=NtCreateFile(out child,0x00100000U | (directory?1U:0x80000000U),ref a,out io,
                IntPtr.Zero,0,1,1,0x00200020U | (directory?1U:0x40U),IntPtr.Zero,0);
            if(status<0 || child.IsInvalid) { child.Dispose(); throw Refused(); }
            return child;
        } finally { if(descriptor!=IntPtr.Zero) Marshal.FreeHGlobal(descriptor); Marshal.FreeHGlobal(text); }
    }
    FileId Identity(SafeFileHandle h) {
        FileId id;
        // FileIdInfo is mandatory; unsupported filesystems fail closed.
        if(!GetFileInformationByHandleEx(h,18,out id,24) || id.volume!=volume || (id.low==0 && id.high==0)) throw Refused();
        return id;
    }
    internal static string IdText(ulong low,ulong high) { return low.ToString("x16")+high.ToString("x16"); }
    void DirectoryValid(SafeFileHandle h) {
        Info i;
        if(!GetFileInformationByHandle(h,out i) || i.volume!=volume || (i.attributes&0x410)!=0x10 || GetFileType(h)!=1) throw Refused();
        Identity(h);
    }
    SafeFileHandle OpenParent(string path) {
        if(disposed || !ReplayController.LaunchSpec.Absolute(path) || !path.StartsWith(root+"\\",StringComparison.OrdinalIgnoreCase)) throw Refused();
        string[] pieces=path.Substring(3).Split('\\');
        // Volume root is only a transient bootstrap handle, with full sharing.
        // It is closed as soon as its first child has been opened.
        SafeFileHandle parent=CreateFileW(path.Substring(0,3),1,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
        try {
            if(parent.IsInvalid) throw Refused();
            DirectoryValid(parent);
            for(int n=0;n<pieces.Length-1;n++) {
                var child=Child(parent,pieces[n],true);
                ancestors.Add(child); DirectoryValid(child);
                if(n==0) parent.Dispose();
                parent=child;
            }
            return parent;
        } catch { parent.Dispose(); throw; }
    }
    FileStream OpenLeaf(string path) {
        var parent=OpenParent(path);
        var handle=Child(parent,Path.GetFileName(path),false);
        try {
            var stream=new FileStream(handle,FileAccess.Read,4096,false);
            files.Add(stream); return stream;
        } catch { handle.Dispose(); throw; }
    }
    internal static bool LinksUnchanged(uint before,uint after) { return before==1 && after==1; }
    Info Inspect(FileStream stream,long limit) {
        Info info;
        if(!GetFileInformationByHandle(stream.SafeFileHandle,out info)) throw Refused();
        long size=((long)info.sizeHigh<<32)|info.sizeLow;
        if(info.volume!=volume || !MetadataValid(info.attributes,GetFileType(stream.SafeFileHandle),info.links,size,limit)) throw Refused();
        return info;
    }
    byte[] ReadVerified(FileStream stream,string hash,int limit,string expectedId) {
        Info before=Inspect(stream,limit);
        FileId first=Identity(stream.SafeFileHandle);
        if(IdText(first.low,first.high)!=expectedId) throw Refused();
        byte[] bytes=new byte[(int)before.sizeLow];
        stream.Position=0; int read=0;
        while(read<bytes.Length) {
            int n=stream.Read(bytes,read,bytes.Length-read);
            if(n==0) throw Refused();
            read+=n;
        }
        if(stream.ReadByte()!=-1 || !HashMatches(bytes,hash,limit)) throw Refused();
        Info after=Inspect(stream,limit); // includes links==1 AFTER hashing
        if(!LinksUnchanged(before.links,after.links)) throw Refused();
        FileId last=Identity(stream.SafeFileHandle);
        if(first.volume!=last.volume || first.low!=last.low || first.high!=last.high) throw Refused();
        if(before.idHigh!=after.idHigh || before.idLow!=after.idLow || before.sizeLow!=after.sizeLow || before.sizeHigh!=after.sizeHigh) throw Refused();
        return bytes;
    }
    // expected hash and identity MUST come from an externally pinned admission
    // manifest. This primitive never generates, trusts or bootstraps a manifest.
    internal byte[] ReadPinned(string path,string externalHash,int limit,string expectedId) {
        try { return ReadVerified(OpenLeaf(path),externalHash,limit,expectedId); }
        catch { Dispose(); throw; }
    }
    public void Dispose() {
        if(disposed) return;
        disposed=true;
        for(int i=files.Count-1;i>=0;i--) files[i].Dispose();
        for(int i=ancestors.Count-1;i>=0;i--) ancestors[i].Dispose();
    }
}
