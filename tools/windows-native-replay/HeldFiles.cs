// SOURCE ONLY. No public entry point; compiling this file invokes no native API.
// This is a file-pin primitive, not manifest admission or a process controller.
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
    void OpenAncestors(string path) {
        if(disposed || !ReplayController.LaunchSpec.Absolute(path) || !path.StartsWith(root+"\\",StringComparison.OrdinalIgnoreCase)) throw Refused();
        string current=path.Substring(0,3);
        string[] pieces=path.Substring(3).Split('\\');
        for(int i=0;i<pieces.Length;i++) {
            // Root first, then each parent. Leaf is opened by OpenLeaf.
            if(i>0) current=Path.Combine(current,pieces[i-1]);
            var h=CreateFileW(current,0,1,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
            if(h.IsInvalid) { h.Dispose(); throw Refused(); }
            ancestors.Add(h);
            Info info;
            if(!GetFileInformationByHandle(h,out info) || info.volume!=volume || (info.attributes&0x400)!=0
                || (info.attributes&0x10)==0 || GetFileType(h)!=1) throw Refused();
        }
    }
    FileStream OpenLeaf(string path,bool create) {
        OpenAncestors(path);
        // FILE_SHARE_READ only: a held leaf cannot be overwritten or replaced.
        // Copy creation retains this exact handle; it is never closed/reopened
        // between the write, verification, and the caller's eventual teardown.
        var handle=CreateFileW(path,create?0xc0000000U:0x80000000U,1,IntPtr.Zero,create?1U:3U,0x00200000,IntPtr.Zero);
        if(handle.IsInvalid) { handle.Dispose(); throw Refused(); }
        try {
            var stream=new FileStream(handle,create?FileAccess.ReadWrite:FileAccess.Read,4096,false);
            files.Add(stream); return stream;
        } catch { handle.Dispose(); throw; }
    }
    Info Inspect(FileStream stream,long limit) {
        Info info;
        if(!GetFileInformationByHandle(stream.SafeFileHandle,out info)) throw Refused();
        long size=((long)info.sizeHigh<<32)|info.sizeLow;
        if(info.volume!=volume || !MetadataValid(info.attributes,GetFileType(stream.SafeFileHandle),info.links,size,limit)) throw Refused();
        return info;
    }
    byte[] ReadVerified(FileStream stream,string hash,int limit,ulong expectedId) {
        Info before=Inspect(stream,limit);
        if((((ulong)before.idHigh<<32)|before.idLow)!=expectedId) throw Refused();
        byte[] bytes=new byte[(int)before.sizeLow];
        stream.Position=0; int read=0;
        while(read<bytes.Length) {
            int n=stream.Read(bytes,read,bytes.Length-read);
            if(n==0) throw Refused();
            read+=n;
        }
        if(stream.ReadByte()!=-1 || !HashMatches(bytes,hash,limit)) throw Refused();
        Info after=Inspect(stream,limit);
        if(before.idHigh!=after.idHigh || before.idLow!=after.idLow || before.sizeLow!=after.sizeLow || before.sizeHigh!=after.sizeHigh) throw Refused();
        return bytes;
    }
    // expected hash and identity MUST come from an externally pinned admission
    // manifest. This primitive never generates, trusts or bootstraps a manifest.
    internal byte[] ReadPinned(string path,string externalHash,int limit,ulong expectedId) {
        try { return ReadVerified(OpenLeaf(path,false),externalHash,limit,expectedId); }
        catch { Dispose(); throw; }
    }
    internal void CopyPinned(string source,string destination,string externalHash,int limit,ulong expectedSourceId) {
        try {
            // Inspect rejects original Cargo artifacts with nlink=2 BEFORE copy.
            byte[] bytes=ReadPinned(source,externalHash,limit,expectedSourceId);
            var copy=OpenLeaf(destination,true);
            copy.Write(bytes,0,bytes.Length); copy.Flush(true);
            Info info=Inspect(copy,limit);
            ReadVerified(copy,externalHash,limit,((ulong)info.idHigh<<32)|info.idLow);
        } catch { Dispose(); throw; }
    }
    public void Dispose() {
        if(disposed) return;
        disposed=true;
        for(int i=files.Count-1;i>=0;i--) files[i].Dispose();
        for(int i=ancestors.Count-1;i>=0;i--) ancestors[i].Dispose();
    }
}
