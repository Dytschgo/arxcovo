// Strict, bounded build-artifact manifest admission. This does not grant run permission.
using System;
using System.Collections.Generic;
using System.Text;
using System.Text.Json;
using System.IO;

internal sealed class Admission : IDisposable {
    internal sealed class Pin {
        internal string role,path,sha256,id,blob;
        internal int size;
    }
    internal readonly string Root,Libtest;
    readonly string helper,temp,systemRoot;
    readonly HeldFiles held;
    readonly List<Pin> pins;
    internal static readonly string[] Sources={
        "crates/arxd/src/development/windows.rs", "Cargo.lock", "rust-toolchain.toml",
        "tools/windows-native-replay/child/Cargo.toml", "tools/windows-native-replay/child/Cargo.lock",
        "tools/windows-native-replay/child/src/main.rs", "tools/windows-native-replay/Invoke-BoundedReplay.ps1",
        "tools/windows-native-replay/Controller.cs", "tools/windows-native-replay/HeldFiles.cs",
        "tools/windows-native-replay/NativeJob.cs", "tools/windows-native-replay/Admission.cs",
        "tools/windows-native-replay/Prepare-Provenance.py", "tools/windows-native-replay/Check-Source.py",
        "tools/windows-native-replay/selectors.json", "tools/windows-native-replay/Test-PureController.ps1",
        "tools/windows-native-replay/PINNING.md"
    };
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
    internal static List<Pin> Parse(byte[] bytes,string externalSha,string reviewedCommit,string root,IDictionary<string,string> reviewedBlobs) {
        Need(bytes!=null && bytes.Length<=65536 && HeldFiles.HashMatches(bytes,externalSha,65536));
        using(var doc=JsonDocument.Parse(bytes,new JsonDocumentOptions {MaxDepth=4,AllowTrailingCommas=false,CommentHandling=JsonCommentHandling.Disallow})) {
            var e=doc.RootElement;
            Keys(e,"schema","reviewed_commit","root","helper_directory","temp","system_root","target","profile","debug","features","toolchain","compiler_commit","cargo_version","pins");
            Need(e.GetProperty("schema").GetInt32()==1 && Hex(reviewedCommit,40) && Text(e,"reviewed_commit",40)==reviewedCommit);
            Need(Text(e,"root",240)==root && ReplayController.LaunchSpec.Absolute(root));
            Need(Text(e,"target",64)=="x86_64-pc-windows-msvc" && Text(e,"profile",8)=="dev" && Text(e,"debug",1)=="0" && Text(e,"features",16)=="default");
            Need(Text(e,"toolchain",16)=="1.95.0" && Text(e,"compiler_commit",40)=="59807616e1fa2540724bfbac14d7976d7e4a3860" && Text(e,"cargo_version",80)=="1.95.0 (f2d3ce0bd7f24a49f8f72d9000448f8838c4e850)");
            foreach(string key in new[]{"helper_directory","temp","system_root"}) Need(Inside(root,Text(e,key,240)));
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
                    Need(p.path==root+"\\"+relative.Replace('/','\\') && reviewedBlobs.ContainsKey(relative) && reviewedBlobs[relative]==p.blob && Hex(p.blob,40));
                } else Need(p.blob=="artifact");
                Need(!p.path.EndsWith("\\run-manifest.json",StringComparison.OrdinalIgnoreCase));
                result.Add(p);
            }
            Need(roles.Count==0);
            return result;
        }
    }
    static bool Inside(string root,string p) { return ReplayController.LaunchSpec.Absolute(p) && p.StartsWith(root+"\\",StringComparison.OrdinalIgnoreCase); }
    internal Admission(HeldFiles held,byte[] manifest,string externalSha,string commit,string root,IDictionary<string,string> blobs,bool cleanCheckout) {
        Need(cleanCheckout); this.held=held; Root=root;
        pins=Parse(manifest,externalSha,commit,root,blobs);
        using(var d=JsonDocument.Parse(manifest)) {
            helper=Text(d.RootElement,"helper_directory",240); temp=Text(d.RootElement,"temp",240); systemRoot=Text(d.RootElement,"system_root",240);
        }
        Libtest=pins.Find(p=>p.role=="libtest").path;
        Need(pins.Find(p=>p.role=="helper").path==helper+"\\cargo.exe");
        Revalidate();
    }
    internal void Revalidate() {
        foreach(var p in pins) {
            byte[] bytes=held.ReadPinned(p.path,p.sha256,p.size,p.id);
            Need(bytes.Length==p.size);
            if(p.role.StartsWith("source:",StringComparison.Ordinal)) Need(Array.IndexOf(bytes,(byte)13)<0);
        }
        // Directory membership cannot currently be sealed against concurrent
        // insertion by this adapter. Fail before any process launch until the
        // handle-based directory exclusion is implemented and independently reviewed.
        throw new InvalidOperationException("helper directory exclusion unproved");
    }
    internal byte[] Environment(string[] args) {
        var env=new Dictionary<string,string>{{"PATH",helper},{"TEMP",temp},{"TMP",temp},{"SystemRoot",systemRoot}};
        return ReplayController.LaunchSpec.EnvironmentBlock(Libtest,Root,helper,temp,systemRoot,args,env);
    }
    public void Dispose() { held.Dispose(); }
}
