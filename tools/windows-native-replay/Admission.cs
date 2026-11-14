// Strict, bounded build-artifact manifest admission. This does not grant run permission.
using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using System.IO;
using System.Security.Cryptography;
using System.Globalization;

internal interface IAdmissionFiles {
    byte[] ReadPinned(string path,string hash,int limit,string id);
    IAdmissionDirectory ExclusiveDirectory(string path);
    void HoldEnvironment(string temp,string systemRoot);
}
internal interface IAdmissionDirectory {
    HeldFiles.DirectoryEntry[] Scan();
    byte[] ReadLeaf(Admission.Pin pin);
}
internal sealed class Admission : IDisposable {
    internal sealed class Pin {
        internal string role,path,sha256,id,blob;
        internal int size;
    }
    internal readonly string Root,Libtest;
    readonly string helper,temp,systemRoot;
    readonly HeldFiles held;
    readonly List<Pin> pins;
    static readonly string[] sourceNames={
        "crates/arxd/src/development/windows.rs", "Cargo.lock", "rust-toolchain.toml",
        "tools/windows-native-replay/child/Cargo.toml", "tools/windows-native-replay/child/Cargo.lock",
        "tools/windows-native-replay/child/src/main.rs", "tools/windows-native-replay/Invoke-BoundedReplay.ps1",
        "tools/windows-native-replay/Controller.cs", "tools/windows-native-replay/HeldFiles.cs",
        "tools/windows-native-replay/NativeJob.cs", "tools/windows-native-replay/Admission.cs",
        "tools/windows-native-replay/Prepare-Provenance.py", "tools/windows-native-replay/Check-Source.py",
        "tools/windows-native-replay/selectors.json", "tools/windows-native-replay/Test-PureController.ps1",
        "tools/windows-native-replay/PINNING.md"
    };
    internal static string[] Sources { get { return (string[])sourceNames.Clone(); } }
    static void Need(bool ok) { if(!ok) throw new InvalidOperationException("manifest refused"); }
    static bool Hex(string s,int n) {
        if(s==null || s.Length!=n) return false;
        foreach(char c in s) if(!((c>='0' && c<='9') || (c>='a' && c<='f'))) return false;
        return true;
    }
    static void Keys(JsonElement e,params string[] names) {
        Need(e.ValueKind==JsonValueKind.Object);
        var set=new HashSet<string>(names,StringComparer.Ordinal);
        foreach(var p in e.EnumerateObject()) Need(set.Remove(p.Name));
        Need(set.Count==0);
    }
    static string Text(JsonElement e,string key,int max) {
        var p=e.GetProperty(key); Need(p.ValueKind==JsonValueKind.String);
        string value=p.GetString(); Need(value!=null && value.Length>0 && value.Length<=max && value.IndexOf('\0')<0); return value;
    }
    internal static List<Pin> Parse(byte[] bytes,string externalSha,string reviewedCommit,string root) {
        Need(bytes!=null && bytes.Length<=65536 && HeldFiles.HashMatches(bytes,externalSha,65536));
        using(var doc=JsonDocument.Parse(bytes,new JsonDocumentOptions {MaxDepth=4,AllowTrailingCommas=false,CommentHandling=JsonCommentHandling.Disallow})) {
            var e=doc.RootElement;
            Keys(e,"schema","runtime_authority","reviewed_commit","root","helper_directory","temp","system_root","target","profile","debug","features","toolchain","compiler_commit","cargo_version","pins");
            Need(e.GetProperty("runtime_authority").ValueKind==JsonValueKind.False);
            Need(e.GetProperty("schema").GetInt32()==1 && Hex(reviewedCommit,40) && Text(e,"reviewed_commit",40)==reviewedCommit);
            Need(Text(e,"root",240)==root && ReplayController.LaunchSpec.Absolute(root));
            Need(Text(e,"target",64)=="x86_64-pc-windows-msvc" && Text(e,"profile",8)=="dev" && Text(e,"debug",1)=="0" && Text(e,"features",16)=="default");
            Need(Text(e,"toolchain",16)=="1.95.0" && Text(e,"compiler_commit",40)=="59807616e1fa2540724bfbac14d7976d7e4a3860" && Text(e,"cargo_version",80)=="1.95.0 (f2d3ce0bd7f24a49f8f72d9000448f8838c4e850)");
            Need(SystemRootSeparate(root,Text(e,"helper_directory",240),Text(e,"temp",240),Text(e,"system_root",240),new string[0]));
            var roles=new HashSet<string>(StringComparer.Ordinal);
            foreach(string source in Sources) roles.Add("source:"+source);
            foreach(string role in new[]{"controller","cargo","helper","libtest","selectors","provenance","workspace_lock","helper_lock"}) roles.Add(role);
            var a=e.GetProperty("pins"); Need(a.ValueKind==JsonValueKind.Array && a.GetArrayLength()==roles.Count);
            var result=new List<Pin>();
            foreach(var v in a.EnumerateArray()) {
                Keys(v,"role","path","sha256","id128","size","blob");
                var p=new Pin {role=Text(v,"role",200),path=Text(v,"path",240),sha256=Text(v,"sha256",64),id=Text(v,"id128",32),size=v.GetProperty("size").GetInt32(),blob=Text(v,"blob",40)};
                Need(roles.Remove(p.role) && Inside(root,p.path) && Hex(p.sha256,64) && Hex(p.id,32) && p.id!=new string('0',32) && p.size>0 && p.size<=67108864);
                if(p.role.StartsWith("source:",StringComparison.Ordinal)) {
                    string relative=p.role.Substring(7);
                    Need(p.path==root+"\\"+relative.Replace('/','\\') && Hex(p.blob,40));
                } else Need(p.blob=="artifact");
                if(p.role=="provenance") Need(p.size<=65536);
                if(p.role.StartsWith("source:",StringComparison.Ordinal)) Need(p.size<=2097152);
                Need(!p.path.EndsWith("\\run-manifest.json",StringComparison.OrdinalIgnoreCase));
                result.Add(p);
            }
            Need(roles.Count==0);
            return result;
        }
    }
    internal static bool Inside(string root,string p) { return ReplayController.LaunchSpec.Absolute(p) && p.StartsWith(root+"\\",StringComparison.OrdinalIgnoreCase); }
    // The coordinator supplies this root pin OUTSIDE both JSON documents. It is
    // source identity evidence only, never execution permission.
    readonly string trustHash,dispositionId;
    internal Admission(HeldFiles held,string externalDispositionSha,string externalDispositionId,string root) {
        this.held=held; Root=root; trustHash=externalDispositionSha; dispositionId=externalDispositionId;
        var facts=Bind(held,trustHash,dispositionId,Root);
        pins=facts.CopyPins(); helper=facts.Helper; temp=facts.Temp; systemRoot=facts.SystemRoot;
        Libtest=pins.Find(p=>p.role=="libtest").path;
        Revalidate();
    }
    internal sealed class Facts {
        readonly List<Pin> values;
        internal readonly string Commit,Helper,Temp,SystemRoot;
        internal Facts(List<Pin> pins,string commit,string helper,string temp,string system) {
            values=ClonePins(pins); Commit=commit; Helper=helper; Temp=temp; SystemRoot=system;
        }
        internal List<Pin> CopyPins() { return ClonePins(values); }
    }
    static List<Pin> ClonePins(List<Pin> pins) {
        return pins.ConvertAll(p=>new Pin {role=p.role,path=p.path,sha256=p.sha256,id=p.id,blob=p.blob,size=p.size});
    }
    static bool Id(string id) { return Hex(id,32) && id!=new string('0',32); }
    internal static byte[] Read(IAdmissionFiles files,string path,string hash,int size,string id) {
        Need(files!=null && Hex(hash,64) && Id(id) && size>0 && size<=67108864);
        byte[] b=files.ReadPinned(path,hash,size,id);
        Need(b!=null && b.Length==size && HeldFiles.HashMatches(b,hash,size));
        return b;
    }
    internal static string GitBlob(byte[] bytes) {
        byte[] header=Encoding.ASCII.GetBytes("blob "+bytes.Length.ToString(CultureInfo.InvariantCulture)+"\0");
        using(var sha=SHA1.Create()) {
            sha.TransformBlock(header,0,header.Length,null,0);
            sha.TransformFinalBlock(bytes,0,bytes.Length);
            return BitConverter.ToString(sha.Hash).Replace("-","").ToLowerInvariant();
        }
    }
    static bool Equal(byte[] a,byte[] b) {
        if(a.Length!=b.Length) return false;
        for(int i=0;i<a.Length;i++) if(a[i]!=b[i]) return false;
        return true;
    }
    static void Alias(List<Pin> pins,Dictionary<string,byte[]> bytes,string role,string source) {
        Pin a=pins.Find(p=>p.role==role),b=pins.Find(p=>p.role=="source:"+source);
        Need(a.path==b.path && a.id==b.id && a.size==b.size && a.sha256==b.sha256 && Equal(bytes[a.role],bytes[b.role]));
    }
    // This is the real binder used by native admission and the in-memory oracle.
    // There is no overload accepting manifest bytes, blob maps or clean flags.
    internal static Facts Bind(IAdmissionFiles files,string externalDispositionSha,string externalDispositionId,string root) {
        Need(files!=null && ReplayController.LaunchSpec.Absolute(root) && Hex(externalDispositionSha,64) && Id(externalDispositionId));
        string basePath=root+"\\tools\\windows-native-replay\\";
        byte[] disposition=files.ReadPinned(basePath+"run-disposition.json",externalDispositionSha,65536,externalDispositionId);
        Need(disposition!=null && disposition.Length>0 && disposition.Length<=65536 && HeldFiles.HashMatches(disposition,externalDispositionSha,65536));
        using(var d=JsonDocument.Parse(disposition,new JsonDocumentOptions {MaxDepth=4})) {
            var e=d.RootElement;
            Keys(e,"schema","runtime_authority","reviewed_commit","root","manifest","source_blobs");
            Need(e.GetProperty("schema").GetInt32()==1 && e.GetProperty("runtime_authority").ValueKind==JsonValueKind.False && Text(e,"root",240)==root);
            string commit=Text(e,"reviewed_commit",40); Need(Hex(commit,40));
            var m=e.GetProperty("manifest"); Keys(m,"sha256","id128","size");
            int size=m.GetProperty("size").GetInt32(); Need(size>0 && size<=65536);
            byte[] manifest=Read(files,basePath+"run-manifest.json",Text(m,"sha256",64),size,Text(m,"id128",32));
            var pins=Parse(manifest,Text(m,"sha256",64),commit,root);
            var blobs=e.GetProperty("source_blobs"); Keys(blobs,Sources);
            var bytes=new Dictionary<string,byte[]>(StringComparer.Ordinal);
            // Keep the existing directory/type/identity checks and both scans.
            using(var md=JsonDocument.Parse(manifest)) {
                var me=md.RootElement;
                string helper=Text(me,"helper_directory",240),temp=Text(me,"temp",240),system=Text(me,"system_root",240);
                RevalidatePins(files,pins,root,helper,temp,system,bytes);
                foreach(string source in Sources) {
                    Pin p=pins.Find(v=>v.role=="source:"+source);
                    Need(Hex(Text(blobs,source,40),40) && p.blob==Text(blobs,source,40) && GitBlob(bytes[p.role])==p.blob);
                }
                foreach(var pin in pins) Need(HeldFiles.HashMatches(bytes[pin.role],pin.sha256,pin.size));
                Alias(pins,bytes,"workspace_lock","Cargo.lock");
                Alias(pins,bytes,"helper_lock","tools/windows-native-replay/child/Cargo.lock");
                Alias(pins,bytes,"selectors","tools/windows-native-replay/selectors.json");
                // Every other role owns its own path; no unreviewed role alias.
                for(int i=0;i<pins.Count;i++) for(int j=0;j<i;j++) {
                    Pin a=pins[i],b=pins[j];
                    if(!string.Equals(a.path,b.path,StringComparison.OrdinalIgnoreCase)) continue;
                    string pair=a.role+"|"+b.role,reverse=b.role+"|"+a.role;
                    var allowed=new[]{"workspace_lock|source:Cargo.lock","helper_lock|source:tools/windows-native-replay/child/Cargo.lock","selectors|source:tools/windows-native-replay/selectors.json"};
                    Need(Array.IndexOf(allowed,pair)>=0 || Array.IndexOf(allowed,reverse)>=0);
                }
                using(var pd=JsonDocument.Parse(bytes["provenance"],new JsonDocumentOptions {MaxDepth=4})) {
                    var pe=pd.RootElement;
                    Keys(pe,"schema","source_commit_claim","runtime_authority","native_executed","source_sha256","helper_sha256");
                    Need(pe.GetProperty("schema").GetInt32()==1 && Text(pe,"source_commit_claim",40)==commit
                        && pe.GetProperty("runtime_authority").ValueKind==JsonValueKind.False && pe.GetProperty("native_executed").ValueKind==JsonValueKind.False);
                    var hashes=pe.GetProperty("source_sha256"); Keys(hashes,Sources);
                    foreach(string source in Sources) Need(HeldFiles.HashMatches(bytes["source:"+source],Text(hashes,source,64),67108864));
                    Need(HeldFiles.HashMatches(bytes["helper"],Text(pe,"helper_sha256",64),67108864));
                }
                return new Facts(pins,commit,helper,temp,system);
            }
        }
    }
    internal static bool SystemRootSeparate(string root,string helper,string temp,string system,IEnumerable<string> paths) {
        if(!ReplayController.LaunchSpec.Absolute(root) || !Inside(root,helper) || !Inside(root,temp)
            || !ReplayController.LaunchSpec.Absolute(system) || string.Equals(root,system,StringComparison.OrdinalIgnoreCase)
            || Inside(root,system) || Inside(system,root) || paths==null) return false;
        foreach(string path in paths) if(!Inside(root,path)) return false;
        return true;
    }
    internal static void RevalidatePins(IAdmissionFiles files,List<Pin> pins,string root,string helper,string temp,string system,Dictionary<string,byte[]> content=null) {
        Need(files!=null && pins!=null);
        Need(SystemRootSeparate(root,helper,temp,system,pins.ConvertAll(p=>p.path)));
        Pin helperPin=pins.Find(p=>p.role=="helper"), image=pins.Find(p=>p.role=="libtest");
        Need(helperPin!=null && image!=null && helperPin.path==helper+"\\cargo.exe");
        string imageDirectory=Path.GetDirectoryName(image.path);
        // Each executable directory has exactly one pinned leaf. Sharing a directory
        // would require a different allowlist contract and is refused here.
        Need(!string.Equals(helper,imageDirectory,StringComparison.OrdinalIgnoreCase));
        byte[] helperBytes=VerifyExclusive(files.ExclusiveDirectory(helper),helperPin);
        byte[] imageBytes=VerifyExclusive(files.ExclusiveDirectory(imageDirectory),image);
        if(content!=null) { content.Add(helperPin.role,helperBytes); content.Add(image.role,imageBytes); }
        files.HoldEnvironment(temp,system);
        foreach(var p in pins) {
            if(p==helperPin || p==image) continue;
            byte[] bytes=content==null?files.ReadPinned(p.path,p.sha256,p.size,p.id):Read(files,p.path,p.sha256,p.size,p.id);
            if(content!=null) content.Add(p.role,bytes);
            Need(bytes.Length==p.size);
            if(p.role.StartsWith("source:",StringComparison.Ordinal)) Need(Array.IndexOf(bytes,(byte)13)<0);
        }
    }
    static byte[] VerifyExclusive(IAdmissionDirectory directory,Pin pin) {
        var first=directory.Scan();
        string leaf=Path.GetFileName(pin.path);
        Need(HeldFiles.ExclusiveNames(first,leaf));
        byte[] leafBytes=directory.ReadLeaf(pin);
        Need(leafBytes.Length==pin.size);
        var second=directory.Scan();
        Need(HeldFiles.ExclusiveNames(second,leaf) && HeldFiles.SameScan(first,second));
        foreach(var entry in second) if(entry.Name==leaf) Need(HeldFiles.IdText(entry.Low,entry.High)==pin.id);
        return leafBytes;
    }
    internal void Revalidate() {
        Bind(held,trustHash,dispositionId,Root);
        RevalidateBoundary(held,pins,Root,helper,temp,systemRoot);
    }
    internal static void RevalidateBoundary(IAdmissionFiles files,List<Pin> pins,string root,string helper,string temp,string system) {
        RevalidatePins(files,pins,root,helper,temp,system);
        // Native query semantics remain UNRUN. Transport, grammar and
        // deadline supervision are separate unfinished gates, never caller flags.
        throw new InvalidOperationException("remaining admission gates unproved");
    }
    internal byte[] Environment(string[] args) {
        var env=new Dictionary<string,string>{{"PATH",helper},{"TEMP",temp},{"TMP",temp},{"SystemRoot",systemRoot}};
        return ReplayController.LaunchSpec.EnvironmentBlock(Libtest,Root,helper,temp,systemRoot,args,env);
    }
    public void Dispose() { held.Dispose(); }
}
