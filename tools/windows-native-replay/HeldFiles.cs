// SOURCE ONLY. No public entry point; compiling this file invokes no native API.
// Used only by the dormant manifest adapter. No copy or hardlink fallback.
using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using Microsoft.Win32.SafeHandles;

internal sealed class HeldFiles : IDisposable, IAdmissionFiles {
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
    readonly string root,systemRoot,systemId;
    readonly Dictionary<string,SafeFileHandle> directories=new Dictionary<string,SafeFileHandle>(StringComparer.OrdinalIgnoreCase);
    bool disposed;
    internal HeldFiles(uint approvedVolume,string approvedRoot,string approvedSystemRoot,string approvedSystemId) {
        if(approvedVolume==0 || !ReplayController.LaunchSpec.Absolute(approvedRoot)) throw new ArgumentException("root");
        if(!Admission.SystemRootSeparate(approvedRoot,approvedRoot+"\\bin",approvedRoot+"\\tmp",approvedSystemRoot,new string[0])
            || approvedSystemId==null || approvedSystemId.Length!=32 || approvedSystemId==new string('0',32)) throw new ArgumentException("system pin");
        foreach(char c in approvedSystemId) if(!((c>='0' && c<='9') || (c>='a' && c<='f'))) throw new ArgumentException("system pin");
        volume=approvedVolume; root=approvedRoot; systemRoot=approvedSystemRoot; systemId=approvedSystemId;
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
            // no name-based reopening and no write/delete sharing. Directory share
            // read fences ADD_FILE, not DELETE_CHILD or WRITE_ATTRIBUTES, and
            // cannot freeze an unheld sibling. Two complete allowlist scans plus
            // the held sole leaf are required; the directory handle alone is not proof.
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
    SafeFileHandle OpenParent(string path,bool environment=false) {
        if(disposed || !ReplayController.LaunchSpec.Absolute(path)
            || (!Admission.Inside(root,path) && !(environment && path==systemRoot+"\\held"))) throw Refused();
        string[] pieces=path.Substring(3).Split('\\');
        // Volume root is only a transient bootstrap handle, with full sharing.
        // It is closed as soon as its first child has been opened.
        SafeFileHandle parent=CreateFileW(path.Substring(0,3),1,7,IntPtr.Zero,3,0x02200000,IntPtr.Zero);
        try {
            if(parent.IsInvalid) throw Refused();
            DirectoryValid(parent);
            for(int n=0;n<pieces.Length-1;n++) {
                string key=path.Substring(0,3)+string.Join("\\",pieces,0,n+1);
                SafeFileHandle child;
                if(!directories.TryGetValue(key,out child)) {
                    child=Child(parent,pieces[n],true);
                    ancestors.Add(child); directories.Add(key,child);
                }
                DirectoryValid(child);
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
    public byte[] ReadPinned(string path,string externalHash,int limit,string expectedId) {
        try { return ReadVerified(OpenLeaf(path),externalHash,limit,expectedId); }
        catch { Dispose(); throw; }
    }
    internal sealed class DirectoryEntry {
        internal readonly string Name;
        internal readonly uint Attributes,ReparseTag;
        internal readonly ulong Low,High;
        internal DirectoryEntry(string name,uint attributes,uint tag,ulong low,ulong high) {
            Name=name; Attributes=attributes; ReparseTag=tag; Low=low; High=high;
        }
    }
    internal static bool ExclusiveNames(DirectoryEntry[] entries,string leaf) {
        if(entries==null || entries.Length!=3 || string.IsNullOrEmpty(leaf) || leaf=="." || leaf=="..") return false;
        var names=new HashSet<string>(StringComparer.Ordinal);
        foreach(var e in entries) {
            if(e==null || e.Name==null || !names.Add(e.Name) || (e.Attributes&0x400)!=0 || e.ReparseTag!=0 || (e.Low==0 && e.High==0)) return false;
            if(e.Name=="." || e.Name=="..") { if((e.Attributes&0x10)==0) return false; }
            else if(e.Name!=leaf || (e.Attributes&0x10)!=0) return false;
        }
        return names.Contains(".") && names.Contains("..") && names.Contains(leaf);
    }
    internal static bool SameScan(DirectoryEntry[] first,DirectoryEntry[] second) {
        if(first==null || second==null || first.Length!=second.Length) return false;
        foreach(var a in first) {
            int matches=0;
            foreach(var b in second) if(a.Name==b.Name && a.Attributes==b.Attributes && a.ReparseTag==b.ReparseTag && a.Low==b.Low && a.High==b.High) matches++;
            if(matches!=1) return false;
        }
        return true;
    }
    [DllImport("ntdll.dll", ExactSpelling=true)]
    static extern int NtQueryDirectoryFile(SafeFileHandle directory,IntPtr ev,IntPtr apc,IntPtr context,
        out IoStatus io,IntPtr buffer,uint length,int informationClass,
        [MarshalAs(UnmanagedType.U1)] bool single,IntPtr name,[MarshalAs(UnmanagedType.U1)] bool restart);
    static DirectoryEntry[] Scan(SafeFileHandle directory) {
        // FILE_ID_EXTD_DIR_INFORMATION (60), one record per call. Unsupported
        // class, overflow, incomplete record, or failure to exhaust refuses.
        IntPtr buffer=Marshal.AllocHGlobal(1024);
        try {
            var entries=new List<DirectoryEntry>();
            for(int call=0;call<4;call++) {
                IoStatus io;
                int status=NtQueryDirectoryFile(directory,IntPtr.Zero,IntPtr.Zero,IntPtr.Zero,out io,buffer,1024,60,true,IntPtr.Zero,call==0);
                if(status==unchecked((int)0x80000006)) return entries.ToArray(); // STATUS_NO_MORE_FILES
                ulong count=io.information.ToUInt64();
                if(status!=0 || count<88 || count>1024 || Marshal.ReadInt32(buffer,0)!=0) throw Refused();
                uint length=unchecked((uint)Marshal.ReadInt32(buffer,60));
                if(length==0 || length>510 || length%2!=0 || 88UL+length>count) throw Refused();
                entries.Add(new DirectoryEntry(Marshal.PtrToStringUni(IntPtr.Add(buffer,88),(int)length/2),
                    unchecked((uint)Marshal.ReadInt32(buffer,56)),unchecked((uint)Marshal.ReadInt32(buffer,68)),
                    unchecked((ulong)Marshal.ReadInt64(buffer,72)),unchecked((ulong)Marshal.ReadInt64(buffer,80))));
            }
            throw Refused();
        } finally { Marshal.FreeHGlobal(buffer); }
    }
    sealed class Exclusive : IAdmissionDirectory {
        readonly HeldFiles owner;
        readonly SafeFileHandle directory;
        internal Exclusive(HeldFiles owner,SafeFileHandle directory) { this.owner=owner; this.directory=directory; }
        public DirectoryEntry[] Scan() { owner.DirectoryValid(directory); return HeldFiles.Scan(directory); }
        public byte[] ReadLeaf(Admission.Pin pin) {
            var handle=Child(directory,Path.GetFileName(pin.path),false);
            FileStream stream;
            try { stream=new FileStream(handle,FileAccess.Read,4096,false); }
            catch { handle.Dispose(); throw; }
            owner.files.Add(stream);
            return owner.ReadVerified(stream,pin.sha256,pin.size,pin.id);
        }
    }
    public IAdmissionDirectory ExclusiveDirectory(string path) {
        if(!Admission.Inside(root,path)) throw Refused();
        return new Exclusive(this,OpenParent(path+"\\held"));
    }
    public void HoldEnvironment(string temp,string system) {
        if(!Admission.Inside(root,temp) || system!=systemRoot) throw Refused();
        OpenParent(temp+"\\held");
        var h=OpenParent(system+"\\held",true);
        var id=Identity(h);
        if(IdText(id.low,id.high)!=systemId) throw Refused();
    }
    // Handles are retained, including the scanned directories and relative leaf
    // handles. Future launch ownership must retain this object until process exit
    // AND Job active zero; current admission always refuses before launch.
    public void Dispose() {
        if(disposed) return;
        disposed=true;
        for(int i=files.Count-1;i>=0;i--) files[i].Dispose();
        for(int i=ancestors.Count-1;i>=0;i--) ancestors[i].Dispose();
    }
}
