// Gate 2 source only. PowerShell and registration guards remain unconditional.
// Pure contracts below cannot grant authority or create a process.
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;
using System.Text;

internal static class ReplayController {
    internal struct Row {
        internal readonly int consumed, creates, resumes;
        internal Row(int n,int c,int r) { consumed=n; creates=c; resumes=r; }
    }
    // Generated projection of selectors.json row_map; Check-Source verifies every entry.
    internal static readonly Row[] Rows = {
        new Row(0,0,0), // 0
        new Row(1,0,0), // 1
        new Row(1,0,0), // 2
        new Row(1,0,0), // 3
        new Row(1,0,0), // 4
        new Row(1,0,0), // 5
        new Row(1,0,0), // 6
        new Row(1,0,0), // 7
        new Row(1,0,0), // 8
        new Row(1,0,0), // 9
        new Row(1,0,0), // 10
        new Row(1,0,0), // 11
        new Row(1,0,0), // 12
        new Row(1,0,0), // 13
        new Row(1,0,0), // 14
        new Row(1,1,0), // 15
        new Row(1,1,0), // 16
        new Row(1,1,0), // 17
        new Row(1,1,0), // 18
        new Row(1,1,1), // 19
        new Row(1,1,1), // 20
        new Row(1,1,1), // 21
        new Row(1,1,1), // 22
        new Row(1,1,1), // 23
        new Row(1,1,1), // 24
        new Row(1,1,1), // 25
        new Row(1,1,1), // 26
        new Row(1,1,1), // 27
        new Row(1,1,1), // 28
        new Row(1,1,1), // 29
        new Row(1,1,1), // 30
        new Row(1,1,1), // 31
        new Row(1,1,1), // 32
        new Row(0,1,1), // 33
        new Row(0,1,1), // 34
    };
    internal struct Event {
        internal int index, consumed, creates, resumes, clean, stdout, stderr, durationMs;
        internal bool begin;
        internal Event(int index, bool begin, int consumed=0, int creates=0,
            int resumes=0, int clean=1, int stdout=0, int stderr=0, int durationMs=0) {
            this.index=index; this.begin=begin; this.consumed=consumed;
            this.creates=creates; this.resumes=resumes; this.clean=clean;
            this.stdout=stdout; this.stderr=stderr; this.durationMs=durationMs;
        }
    }
    internal sealed class Budget {
        internal int completed, children, resumes;
        int next; bool running, failed; long rowStart, last;
        internal bool Accept(Event e,long elapsedMs) {
            if (failed || elapsedMs<last || elapsedMs>=360000 || e.index!=next || next>=35) return Reject();
            last=elapsedMs;
            if (e.begin) {
                if (running || e.consumed!=0 || e.creates!=0 || e.resumes!=0 || e.clean!=1 || e.stdout!=0 || e.stderr!=0 || e.durationMs!=0) return Reject();
                running=true; rowStart=elapsedMs; return true;
            }
            if (!running || elapsedMs-rowStart>=10000 || e.durationMs<0 || e.durationMs>10000 || e.clean!=1
                || e.consumed<0 || e.consumed>1 || e.creates<0 || e.creates>1 || e.resumes<0 || e.resumes>e.creates
                || e.stdout<0 || e.stdout>1 || e.stderr<0 || e.stderr>1) return Reject();
            Row expected=Rows[e.index];
            if(e.creates!=expected.creates || e.resumes!=expected.resumes || e.consumed!=expected.consumed) return Reject();
            children+=e.creates; resumes+=e.resumes; completed++; next++; running=false;
            return true;
        }
        internal bool Expired(long elapsedMs) {
            if (failed || elapsedMs<last || elapsedMs>=360000 || (running && elapsedMs-rowStart>=10000)) { failed=true; return true; }
            last = elapsedMs;
            return false;
        }
        internal bool Complete { get { return !failed && !running && completed==35 && children==20 && resumes==16; } }
        bool Reject() { failed=true; return false; }
    }

    // Fixed-capacity owned queue: no user callback, blocking read, lock or wait.
    // Overflow latches failure even if the producer ignores the return value.
    internal sealed class RowChannel {
        readonly ConcurrentQueue<Event> queue=new ConcurrentQueue<Event>();
        int count, invalid;
        internal bool Failed { get { return Volatile.Read(ref invalid)!=0; } }
        internal bool Empty { get { return Volatile.Read(ref count)==0 && queue.IsEmpty; } }
        internal void Invalidate() { Interlocked.Exchange(ref invalid,1); }
        internal bool Publish(Event e) {
            if(Interlocked.Increment(ref count)>70) {
                Interlocked.Decrement(ref count); Interlocked.Exchange(ref invalid,1); return false;
            }
            queue.Enqueue(e); return true;
        }
        internal bool TryRead(out Event e) {
            if(!queue.TryDequeue(out e)) return false;
            Interlocked.Decrement(ref count); return true;
        }
    }
    internal sealed class MonitorState {
        internal readonly Budget budget=new Budget();
        long activity;
        bool failed;
        // One event per poll bounds producer influence on the control loop.
        internal bool Poll(RowChannel channel,long now) {
            if(failed || channel.Failed || budget.Expired(now) || now-activity>=10000) { failed=true; return false; }
            Event e;
            if(channel.TryRead(out e)) {
                if(!budget.Accept(e,now)) { failed=true; return false; }
                activity=now;
            }
            return true;
        }
        internal bool Success(RowChannel channel,bool exited,uint exitCode,uint active) {
            return !failed && !channel.Failed && channel.Empty && budget.Complete && exited && exitCode==0 && active==0;
        }
    }
    // Pure cleanup decision used only after the native termination attempt.
    internal static bool CleanupConfirmed(bool terminateSucceeded,uint active,bool processExited,long now,long until) {
        return terminateSucceeded && active==0 && processExited && now<until;
    }
    // Created before either launch; sole owner of Job, channel and clock.
    internal sealed class OwnedSession : IDisposable {
        readonly Stopwatch clock=Stopwatch.StartNew();
        readonly MonitorState state=new MonitorState();
        internal readonly RowChannel channel=new RowChannel();
        readonly FrameDecoder decoder;
        readonly NativeJob job;
        static readonly List<NativeJob> unknownOwners=new List<NativeJob>();
        bool failed, disposed, finished;
        internal OwnedSession() { decoder=new FrameDecoder(channel); job=new NativeJob(); }
        internal bool Poll() { return !failed && state.Poll(channel,clock.ElapsedMilliseconds); }
        internal bool Success(bool exited,uint code,uint active) {
            return Poll() && state.Success(channel,exited,code,active);
        }
        internal bool Run(Admission admission) {
            if(finished || failed || disposed) return false;
            try {
                for(int filter=0;filter<2;filter++) {
                    admission.Revalidate();
                    string[] args={filter==0?LaunchSpec.Kernel:LaunchSpec.Matrix,"--exact","--test-threads=1"};
                    job.Launch(admission.Libtest,admission.Root,args,admission.Environment(args));
                    for(;;) {
                        bool eof; uint code;
                        if(!job.Pump(decoder,out eof) || !Poll() || clock.ElapsedMilliseconds>=358000) return Abort();
                        bool exited=job.Exited(out code);
                        if(exited && eof) {
                            if(code!=0 || job.Active()!=0 || !decoder.Segment(filter==0?2:70)) return Abort();
                            while(!channel.Empty) if(!Poll()) return Abort();
                            if(filter==1 && !Success(true,code,job.Active())) return Abort();
                            break;
                        }
                        Thread.Sleep(1); // only owner waits, no threadpool callback
                    }
                    job.Next();
                }
                finished=true; return true;
            } catch { return Abort(); }
        }
        bool Abort() {
            failed=true; channel.Invalidate();
            if(!job.KillAndConfirm(clock)) {
                lock(unknownOwners) unknownOwners.Add(job); // keep kill-on-close owner, prohibit reuse
            } else { job.Dispose(); disposed=true; }
            return false;
        }
        public void Dispose() {
            if(disposed) return;
            if(!finished) { Abort(); return; }
            job.Dispose(); disposed=true;
        }
    }
    // Fixed binary wire grammar: 10 little-endian uint32 fields, 40 bytes.
    // magic, index, begin, consumed, creates, resumes, clean, stdout, stderr, ms.
    // Single producer only; neither feed nor finish ever waits for input.
    internal sealed class FrameDecoder {
        readonly RowChannel channel;
        readonly byte[] frame=new byte[40];
        int used, frames;
        bool failed, ended;
        internal FrameDecoder(RowChannel channel) { this.channel=channel; }
        internal bool Failed { get { return failed; } }
        internal bool Feed(byte[] bytes,int count) {
            if(failed || ended || bytes==null || count<0 || count>bytes.Length || count>4096) return Fail();
            for(int i=0;i<count;i++) {
                frame[used++]=bytes[i];
                if(used!=40) continue;
                used=0;
                uint[] v=new uint[10];
                for(int j=0;j<10;j++) v[j]=(uint)(frame[j*4] | frame[j*4+1]<<8 | frame[j*4+2]<<16 | frame[j*4+3]<<24);
                if(v[1]!=(uint)(frames/2) || v[2]!=(uint)(1-frames%2)) return Fail();
                if(++frames>70 || v[0]!=0x32585241 || v[1]>=35 || v[2]>1 || v[3]>1 || v[4]>1 || v[5]>v[4]
                    || v[6]!=1 || v[7]>1 || v[8]>1 || v[9]>10000) return Fail();
                if(!channel.Publish(new Event((int)v[1],v[2]==1,(int)v[3],(int)v[4],(int)v[5],(int)v[6],(int)v[7],(int)v[8],(int)v[9]))) return Fail();
            }
            return true;
        }
        internal bool Segment(int expected) {
            if(failed || ended || used!=0 || frames!=expected || (expected!=2 && expected!=70)) return Fail();
            if(expected==70) ended=true;
            return true;
        }
        internal bool Finish() { return Segment(70); }
        bool Fail() { failed=true; channel.Invalidate(); return false; }
    }
    internal sealed class LaunchSpec {
        internal const string Kernel="development::windows::native_replay::kernel_peek_capture";
        internal const string Matrix="development::windows::native_replay::bounded_matrix";
        // Pure grammar only. No arbitrary command line, shell, quoting or env inheritance.
        internal static bool Absolute(string p) {
            if(p==null || p.Length<4 || p.Length>240 || !Char.IsLetter(p[0]) || p[1]!=':' || p[2]!='\\') return false;
            foreach(string component in p.Substring(3).Split('\\')) {
                if(component.Length==0 || component=="." || component==".." || component.EndsWith(".") || component.EndsWith(" ")) return false;
                foreach(char c in component) if(c<32 || "<>:\"/|?*".IndexOf(c)>=0) return false;
            }
            return true;
        }
        // CommandLineToArgvW/MSVC escaping. Always quote, including empty argv.
        internal static string Quote(string value) {
            if(value==null || value.IndexOf('\0')>=0 || value.Length>4096) throw new ArgumentException("argument");
            var result=new StringBuilder("\""); int slashes=0;
            foreach(char c in value) {
                if(c=='\\') { slashes++; continue; }
                if(c=='\"') { result.Append('\\',slashes*2+1); result.Append(c); }
                else { result.Append('\\',slashes); result.Append(c); }
                slashes=0;
            }
            result.Append('\\',slashes*2); result.Append('"'); return result.ToString();
        }
        internal static byte[] EnvironmentBlock(string app,string cwd,string helper,string temp,string systemRoot,string[] args,IDictionary<string,string> env) {
            if(!Valid(app,cwd,helper,temp,systemRoot,args,env)) throw new ArgumentException("launch");
            // Literal order is ordinal-ignore-case sorted. Read only validated values;
            // never call GetEnvironmentVariables or inherit the parent's environment.
            return Encoding.Unicode.GetBytes("PATH="+helper+"\0SystemRoot="+systemRoot+"\0TEMP="+temp+"\0TMP="+temp+"\0\0");
        }
        internal static bool Valid(string app,string cwd,string helper,string temp,string systemRoot,string[] args,IDictionary<string,string> env) {
            if(!Absolute(app) || !Absolute(cwd) || !Absolute(helper) || !Absolute(temp) || !Absolute(systemRoot)) return false;
            if(!app.EndsWith(".exe",StringComparison.OrdinalIgnoreCase)) return false;
            if(args==null || args.Length!=3) return false;
            if(args[0]!=Kernel && args[0]!=Matrix) return false;
            if(args[1]!="--exact" || args[2]!="--test-threads=1") return false;
            if(env==null || env.Count!=4) return false;
            foreach(var pair in env) {
                string expected=pair.Key=="PATH" ? helper : pair.Key=="TEMP" || pair.Key=="TMP" ? temp : pair.Key=="SystemRoot" ? systemRoot : null;
                if(expected==null || pair.Value!=expected || pair.Value.IndexOf('\0')>=0) return false;
            }
            return env.ContainsKey("PATH") && env.ContainsKey("TEMP") && env.ContainsKey("TMP") && env.ContainsKey("SystemRoot");
        }
    }
}

public static class PureControllerChecks {
    static void Check(bool ok,string label) { if(!ok) throw new Exception(label); }
    static ReplayController.Event End(int i) {
        // Independent boundary oracle; mutations to the generated map must fail.
        return new ReplayController.Event(i,false,i>=1 && i<=32?1:0,i>=15?1:0,i>=19?1:0);
    }
    static byte[] Frame(uint index,uint begin) {
        uint[] fields={0x32585241,index,begin,0,0,0,1,0,0,0};
        var result=new byte[40];
        for(int i=0;i<10;i++) for(int j=0;j<4;j++) result[i*4+j]=(byte)(fields[i]>>(8*j));
        return result;
    }
    static string Hash(byte[] b) {
        using(var sha=System.Security.Cryptography.SHA256.Create()) return BitConverter.ToString(sha.ComputeHash(b)).Replace("-","").ToLowerInvariant();
    }
    static bool ManifestAccepted(string json,string expected,string commit) {
        try { Admission.Parse(Encoding.UTF8.GetBytes(json),expected,commit,@"C:\fixture"); return true; }
        catch(InvalidOperationException) { return false; }
    }
    static void ManifestChecks() {
        string commit=new string('a',40), blob=new string('b',40);
        var pins=new List<object>();
        foreach(string source in Admission.Sources) {
            pins.Add(new { role="source:"+source,path=@"C:\fixture\"+source.Replace('/','\\'),sha256=new string('c',64),id128=new string('d',32),size=1,blob=blob });
        }
        foreach(string role in new[]{"controller","cargo","helper","libtest","selectors","provenance","workspace_lock","helper_lock"})
            pins.Add(new { role=role,path=@"C:\fixture\"+role+".bin",sha256=new string('c',64),id128=new string('d',32),size=1,blob="artifact" });
        var document=new {schema=1,runtime_authority=false,reviewed_commit=commit,root=@"C:\fixture",helper_directory=@"C:\fixture\bin",temp=@"C:\fixture\tmp",system_root=@"C:\Windows",
            target="x86_64-pc-windows-msvc",profile="dev",debug="0",features="default",toolchain="1.95.0",compiler_commit="59807616e1fa2540724bfbac14d7976d7e4a3860",cargo_version="1.95.0 (f2d3ce0bd7f24a49f8f72d9000448f8838c4e850)",pins=pins};
        string json=System.Text.Json.JsonSerializer.Serialize(document);
        Check(ManifestAccepted(json,Hash(Encoding.UTF8.GetBytes(json)),commit),"manifest valid");
        Check(!ManifestAccepted(json,new string('f',64),commit),"manifest external mismatch");
        string extra=json.Insert(1,"\"extra\":0,");
        Check(!ManifestAccepted(extra,Hash(Encoding.UTF8.GetBytes(extra)),commit),"manifest extra");
        string self=json.Insert(1,"\"sha256\":\""+Hash(Encoding.UTF8.GetBytes(json))+"\",");
        Check(!ManifestAccepted(self,Hash(Encoding.UTF8.GetBytes(self)),commit),"manifest self hash");
        Check(!ManifestAccepted(json,Hash(Encoding.UTF8.GetBytes(json)),new string('e',40)),"manifest commit mismatch");
        foreach(string badId in new[]{new string('A',32),new string('a',31),new string('a',33),new string('0',32)}) {
            string bad=json.Replace(new string('d',32),badId);
            Check(!ManifestAccepted(bad,Hash(Encoding.UTF8.GetBytes(bad)),commit),"admission: parser id format");
        }
        string nested=json.Replace(System.Text.Json.JsonSerializer.Serialize(@"C:\Windows"),System.Text.Json.JsonSerializer.Serialize(@"C:\fixture\Windows"));
        Check(!ManifestAccepted(nested,Hash(Encoding.UTF8.GetBytes(nested)),commit),"admission: parser nested Windows");
        string outside=json.Replace(System.Text.Json.JsonSerializer.Serialize(@"C:\fixture\controller.bin"),System.Text.Json.JsonSerializer.Serialize(@"C:\outside\controller.bin"));
        Check(!ManifestAccepted(outside,Hash(Encoding.UTF8.GetBytes(outside)),commit),"admission: parser outside artifact");
    }
    // Independent argv reader for the one quoted argument emitted by Quote.
    static string Unquote(string s) {
        Check(s.Length>=2 && s[0]=='"',"quoted start");
        var r=new StringBuilder(); int i=1;
        while(i<s.Length) {
            int n=0; while(i<s.Length && s[i]=='\\') { i++; n++; }
            if(i<s.Length && s[i]=='"') {
                r.Append('\\',n/2);
                if(n%2==1) { r.Append('"'); i++; }
                else { Check(i==s.Length-1,"quoted end"); return r.ToString(); }
            } else { r.Append('\\',n); if(i<s.Length) r.Append(s[i++]); }
        }
        throw new Exception("unterminated");
    }
    public static void Run() {
        ManifestChecks();
        AdmissionPureChecks.Run();
        BindingPureChecks.Run();
        Check(ReplayController.CleanupConfirmed(true,0,true,1,2),"cleanup zero");
        Check(!ReplayController.CleanupConfirmed(false,0,true,1,2),"cleanup kill first");
        Check(!ReplayController.CleanupConfirmed(true,1,true,1,2),"cleanup active");
        Check(!ReplayController.CleanupConfirmed(true,0,false,1,2),"cleanup process");
        Check(!ReplayController.CleanupConfirmed(true,0,true,2,2),"cleanup deadline");
        var partialChannel=new ReplayController.RowChannel();
        var partial=new ReplayController.FrameDecoder(partialChannel);
        partial.Feed(Frame(0,1),40); partial.Feed(Frame(0,0),40); partial.Feed(Frame(1,1),1);
        Check(!partial.Segment(2),"partial EOF");
        var fullChannel=new ReplayController.RowChannel(); var full=new ReplayController.FrameDecoder(fullChannel);
        var fullState=new ReplayController.MonitorState();
        for(uint n=0;n<35;n++) {
            var first=Frame(n,1); var last=Frame(n,0);
            last[12]=(byte)(n>=1 && n<=32?1:0); last[16]=(byte)(n>=15?1:0); last[20]=(byte)(n>=19?1:0);
            Check(full.Feed(first,40) && fullState.Poll(fullChannel,n*2),"stream begin");
            Check(full.Feed(last,40) && fullState.Poll(fullChannel,n*2+1),"stream end");
        }
        Check(full.Finish() && fullState.Success(fullChannel,true,0,0),"stream complete");
        foreach(string arg in new[]{"", "plain", "two words", "GrÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Â ÃƒÂ¢Ã¢â€šÂ¬Ã¢â€žÂ¢ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¼ezi ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¤ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¸ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬Ãƒâ€¦Ã¢â‚¬Å“ÃƒÆ’Ã†â€™Ãƒâ€ Ã¢â‚¬â„¢ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â§ÃƒÆ’Ã†â€™Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â€šÂ¬Ã…Â¡Ãƒâ€šÃ‚Â¬ÃƒÆ’Ã¢â‚¬Å¡Ãƒâ€šÃ‚Â¢ÃƒÆ’Ã†â€™ÃƒÂ¢Ã¢â€šÂ¬Ã‚Â¦ÃƒÆ’Ã‚Â¢ÃƒÂ¢Ã¢â‚¬Å¡Ã‚Â¬ÃƒÂ¢Ã¢â‚¬Å¾Ã‚Â¢", @"C:\a b\", "a\"b", "\\\"", "\\\\"})
            Check(Unquote(ReplayController.LaunchSpec.Quote(arg))==arg,"argument escape");
        Check(!HeldFiles.MetadataValid(0,1,2,4,16),"link2 reject");
        Check(!HeldFiles.LinksUnchanged(1,2),"post hash links");
        Check(!HeldFiles.MetadataValid(0x400,1,1,4,16) && !HeldFiles.MetadataValid(0x10,1,1,4,16)
            && !HeldFiles.MetadataValid(0,2,1,4,16) && !HeldFiles.MetadataValid(0,1,1,17,16),"file bounds");
        Check(HeldFiles.MetadataValid(0,1,1,4,16),"single owned file");
        const string emptyHash="e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        Check(HeldFiles.HashMatches(new byte[0],emptyHash,16),"external hash");
        Check(!HeldFiles.HashMatches(new byte[]{1},emptyHash,16),"held pin mismatch");
        var wireChannel=new ReplayController.RowChannel();
        var decoder=new ReplayController.FrameDecoder(wireChannel);
        byte[] frame=Frame(0,1);
        Check(decoder.Feed(new byte[]{frame[0]},1),"fragment");
        var tail=new byte[39]; Array.Copy(frame,1,tail,0,39);
        Check(decoder.Feed(tail,39) && !decoder.Finish(),"wire complete");
        var wireState=new ReplayController.MonitorState();
        Check(!wireState.Poll(wireChannel,0) && !wireState.Success(wireChannel,true,0,0),"begin alone incomplete");
        foreach(int fault in new[]{0,1,2,3}) {
            wireChannel=new ReplayController.RowChannel(); decoder=new ReplayController.FrameDecoder(wireChannel);
            frame=Frame(0,1);
            if(fault==0) { frame[0]=0; Check(!decoder.Feed(frame,40),"malformed frame"); }
            if(fault==1) { decoder.Feed(frame,40); decoder.Feed(frame,39); Check(!decoder.Finish(),"partial EOF"); }
            if(fault==2) { frame[28]=2; Check(!decoder.Feed(frame,40),"output overflow"); }
            if(fault==3) { Check(decoder.Feed(frame,40),"first frame"); Check(!decoder.Feed(frame,40),"duplicate frame"); }
            Check(!new ReplayController.MonitorState().Poll(wireChannel,0),"wire failure latched");
        }
        var state=new ReplayController.MonitorState();
        var channel=new ReplayController.RowChannel();
        Check(!state.Success(channel,true,0,0),"missing all rows");
        for(int i=0;i<35;i++) {
            Check(channel.Publish(new ReplayController.Event(i,true)),"publish begin");
            Check(state.Poll(channel,i*1000),"begin");
            Check(!state.Success(channel,true,0,0),"open row");
            Check(channel.Publish(End(i)),"publish end");
            Check(state.Poll(channel,i*1000+1),"end");
            if(i<34) Check(!state.Success(channel,true,0,0),"partial exit");
        }
        Check(state.Success(channel,true,0,0),"complete");
        Check(!state.Success(channel,false,0,0) && !state.Success(channel,true,1,0) && !state.Success(channel,true,0,1),"exit accounting");
        foreach(int wrong in new[]{-1,1,35}) {
            var bad=new ReplayController.Budget();
            Check(!bad.Accept(new ReplayController.Event(wrong,true),0),"order");
            Check(!bad.Accept(new ReplayController.Event(0,true),1),"latched");
        }
        channel.Publish(End(34));
        Check(!state.Success(channel,true,0,0),"trailing duplicate");
        Check(!state.Poll(channel,35000),"trailing event rejects");
        var clockOrder=new ReplayController.Budget();
        Check(!clockOrder.Expired(9000) && clockOrder.Expired(8999),"poll clock regression");
        var b=new ReplayController.Budget();
        Check(b.Accept(new ReplayController.Event(0,true),0),"start");
        Check(!b.Accept(new ReplayController.Event(0,true),1),"duplicate");
        b=new ReplayController.Budget(); b.Accept(new ReplayController.Event(0,true),0);
        Check(!b.Accept(new ReplayController.Event(0,false,0,0,0,0),1),"unknown cleanup");
        b=new ReplayController.Budget(); b.Accept(new ReplayController.Event(0,true),0);
        Check(!b.Accept(new ReplayController.Event(0,false,0,1),1),"extra child");
        b=new ReplayController.Budget(); b.Accept(new ReplayController.Event(0,true),0);
        Check(!b.Expired(9999) && b.Expired(10000),"row deadline");
        b=new ReplayController.Budget(); Check(b.Expired(360000),"independent global deadline");
        var stalled=new ReplayController.MonitorState();
        Check(!stalled.Poll(new ReplayController.RowChannel(),10000),"no first event");
        var flood=new ReplayController.RowChannel();
        for(int i=0;i<70;i++) Check(flood.Publish(new ReplayController.Event(0,true)),"capacity");
        Check(!flood.Publish(new ReplayController.Event(0,true)),"overflow");
        Check(!new ReplayController.MonitorState().Poll(flood,0),"overflow fail closed");
        var waiting=new ReplayController.MonitorState(); var empty=new ReplayController.RowChannel();
        Check(waiting.Poll(empty,9999) && !waiting.Poll(empty,10000),"empty nonblocking deadline");
        string app=@"C:\tools\cargo.exe", cwd=@"C:\fixture", helper=@"C:\fixture\bin", temp=@"C:\fixture\temp", root=@"C:\Windows";
        var env=new Dictionary<string,string>{{"PATH",helper},{"TEMP",temp},{"TMP",temp},{"SystemRoot",root}};
        string[] args={ReplayController.LaunchSpec.Kernel,"--exact","--test-threads=1"};
        Check(ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"launch grammar");
        byte[] block=ReplayController.LaunchSpec.EnvironmentBlock(app,cwd,helper,temp,root,args,env);
        Check(Encoding.Unicode.GetString(block)=="PATH="+helper+"\0SystemRoot="+root+"\0TEMP="+temp+"\0TMP="+temp+"\0\0","exact environment block");
        args[0]="arbitrary"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"filter reject"); args[0]=ReplayController.LaunchSpec.Kernel;
        env["OWNER_SECRET"]="synthetic"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"environment reject"); env.Remove("OWNER_SECRET");
        env["PATH"]+=@";C:\other"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"path reject");
        foreach(string path in new[]{"cargo.exe",@"C:\a\..\cargo.exe",@"\\server\cargo.exe",@"C:\a:stream"}) Check(!ReplayController.LaunchSpec.Absolute(path),"absolute reject");
    }
}

// Pure injected metadata/query boundary. No HeldFiles instance or native effect.

internal static class AdmissionPureChecks {
    static void Check(bool value,string label) { if(!value) throw new Exception("admission: "+label); }
    static HeldFiles.DirectoryEntry E(string n,uint a=0,ulong id=3,uint tag=0) {
        return new HeldFiles.DirectoryEntry(n,a,tag,id,0);
    }
    static HeldFiles.DirectoryEntry[] Names(string leaf) { return new[]{E(".",16,1),E("..",16,2),E(leaf)}; }
    sealed class Directory : IAdmissionDirectory {
        internal HeldFiles.DirectoryEntry[] First,Second;
        internal int Scans,Reads;
        internal bool BadSize;
        internal Directory(string leaf) { First=Names(leaf); Second=Names(leaf); }
        public HeldFiles.DirectoryEntry[] Scan() { return ++Scans==1?First:Second; }
        public byte[] ReadLeaf(Admission.Pin pin) { Reads++; return new byte[BadSize?0:pin.size]; }
    }
    sealed class Files : IAdmissionFiles {
        internal readonly Directory Helper=new Directory("cargo.exe"),Image=new Directory("libtest.exe");
        internal int Environment,Reads;
        internal bool BadSource;
        public IAdmissionDirectory ExclusiveDirectory(string path) {
            if(path==@"C:\fixture\bin") return Helper;
            if(path==@"C:\fixture\image") return Image;
            throw new Exception("unexpected synthetic directory");
        }
        public byte[] ReadPinned(string p,string h,int n,string id) { Reads++; return new byte[]{BadSource?(byte)13:(byte)0}; }
        public void HoldEnvironment(string temp,string system) { Environment++; }
    }
    static List<Admission.Pin> Pins() {
        return new List<Admission.Pin> {
            new Admission.Pin {role="helper",path=@"C:\fixture\bin\cargo.exe",size=1,id="00000000000000030000000000000000"},
            new Admission.Pin {role="libtest",path=@"C:\fixture\image\libtest.exe",size=1,id="00000000000000030000000000000000"},
            new Admission.Pin {role="source:sample",path=@"C:\fixture\sample",size=1}
        };
    }
    static bool Refused(Files f,List<Admission.Pin> pins=null,string root=@"C:\fixture",string system=@"C:\Windows") {
        try { Admission.RevalidatePins(f,pins??Pins(),root,@"C:\fixture\bin",@"C:\fixture\tmp",system); return false; }
        catch(InvalidOperationException) { return true; }
    }
    internal static void Run() {
        Check(HeldFiles.ExclusiveNames(Names("cargo.exe"),"cargo.exe"),"names valid");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("cargo.exe"),E("extra.dll")},"cargo.exe"),"extra DLL");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16)},"cargo.exe"),"missing leaf");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("cargo.exe"),E("cargo.exe")},"cargo.exe"),"repeated leaf");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("cargo.exe",0x400)},"cargo.exe"),"reparse name");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("cargo.exe",0,3,1)},"cargo.exe"),"reparse tag");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("wrong.exe")},"cargo.exe"),"wrong leaf");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("cargo.exe",16)},"cargo.exe"),"directory leaf");
        Check(!HeldFiles.ExclusiveNames(new[]{E("."),E("..",16),E("cargo.exe")},"cargo.exe"),"dot type");
        Check(!HeldFiles.ExclusiveNames(new[]{E(".",16),E("..",16),E("cargo.exe",0,0)},"cargo.exe"),"zero id");
        var names=Names("cargo.exe");
        Check(HeldFiles.SameScan(names,new[]{names[2],names[0],names[1]}),"set order");
        Check(!HeldFiles.SameScan(names,new[]{E(".",16,1),E("..",16,2),E("cargo.exe",0,4)}),"changed id");
        Check(!HeldFiles.SameScan(names,new[]{E(".",16,1),E("..",16,2),E("cargo.exe",1)}),"changed attributes");
        Check(!HeldFiles.SameScan(names,new[]{E(".",16,1),E("..",16,2),new HeldFiles.DirectoryEntry("cargo.exe",0,0,3,1)}),"changed high id");
        var f=new Files();
        Check(!Refused(f) && f.Helper.Scans==2 && f.Image.Scans==2 && f.Helper.Reads==1 && f.Image.Reads==1 && f.Environment==1 && f.Reads==1,"revalidate complete");
        f=new Files(); f.Helper.First=new[]{E(".",16),E("..",16),E("extra.dll")};
        Check(Refused(f) && f.Helper.Reads==0,"first scan wired");
        f=new Files(); f.Image.Second=new[]{E(".",16),E("..",16),E("extra.dll")}; Check(Refused(f),"image scan wired");
        f=new Files(); f.Helper.Second=new[]{E(".",16,4),E("..",16,2),E("cargo.exe")}; Check(Refused(f),"changed scans wired");
        f=new Files(); f.Helper.BadSize=true; Check(Refused(f),"leaf size wired");
        var pins=Pins(); pins[1].id=new string('e',32); Check(Refused(new Files(),pins),"leaf id wired");
        f=new Files(); f.BadSource=true; Check(Refused(f),"source LF wired");
        Check(Admission.SystemRootSeparate(@"C:\fixture",@"C:\fixture\bin",@"C:\fixture\tmp",@"C:\Windows",new[]{@"C:\fixture\source",@"C:\fixture\image\libtest.exe"}),"external Windows");
        Check(Refused(new Files(),root:@"C:\"),"volume root");
        Check(Refused(new Files(),system:@"C:\fixture\Windows"),"nested Windows");
        Check(Refused(new Files(),system:@"C:\fixture"),"equal Windows");
        Check(!Admission.SystemRootSeparate(@"C:\Windows\fixture",@"C:\Windows\fixture\bin",@"C:\Windows\fixture\tmp",@"C:\Windows",new string[0]),"workspace under Windows");
        pins=Pins(); pins[2].path=@"C:\outside\source"; Check(Refused(new Files(),pins),"outside source");
        pins=Pins(); pins[1].path=@"C:\outside\libtest.exe"; Check(Refused(new Files(),pins),"outside artifact");
        Check(!Admission.SystemRootSeparate(@"C:\fixture",@"C:\outside\bin",@"C:\fixture\tmp",@"C:\Windows",new string[0]),"outside helper");
        Check(!Admission.SystemRootSeparate(@"C:\fixture",@"C:\fixture\bin",@"C:\outside\tmp",@"C:\Windows",new string[0]),"outside temp");
        Check(!Admission.SystemRootSeparate(@"C:\fixture",@"C:\fixture\bin",@"C:\fixture\tmp",@"C:\Windows",new[]{@"C:\fixture\a\..\escape"}),"unsafe path");
        bool final=false;
        try { Admission.RevalidateBoundary(new Files(),Pins(),@"C:\fixture",@"C:\fixture\bin",@"C:\fixture\tmp",@"C:\Windows"); }
        catch(InvalidOperationException e) { final=e.Message=="remaining admission gates unproved"; }
        Check(final,"final refusal");
    }
}

// Synthetic held-read implementation. No filesystem/native/clock/process calls.
internal static class BindingPureChecks {
    const string Root=@"C:\fixture", Id="00000000000000030000000000000000";
    const string Base=Root+@"\tools\windows-native-replay\";
    static void Check(bool b,string name) { if(!b) throw new Exception("admission: binding "+name); }
    static string Hash(byte[] b) { using(var h=System.Security.Cryptography.SHA256.Create()) return Convert.ToHexString(h.ComputeHash(b)).ToLowerInvariant(); }
    static System.Text.Json.Nodes.JsonNode Node(object o) { return System.Text.Json.Nodes.JsonNode.Parse(System.Text.Json.JsonSerializer.Serialize(o)); }
    static byte[] Bytes(System.Text.Json.Nodes.JsonNode n) { return Encoding.UTF8.GetBytes(n.ToJsonString()); }
    sealed class Fixture : IAdmissionFiles {
        internal readonly Dictionary<string,byte[]> Data=new Dictionary<string,byte[]>(StringComparer.Ordinal);
        internal System.Text.Json.Nodes.JsonNode Manifest,Disposition,Provenance;
        internal string Trust;
        internal int Reads,Scans;
        internal string BadRead;
        internal readonly Dictionary<string,byte[]> Later=new Dictionary<string,byte[]>();
        readonly HashSet<string> Seen=new HashSet<string>();
        internal Fixture() {
            var pins=new List<object>(); var blobs=new Dictionary<string,string>(); var hashes=new Dictionary<string,string>();
            foreach(string source in Admission.Sources) {
                byte[] b=Encoding.UTF8.GetBytes("sample\n"); string path=Root+"\\"+source.Replace('/','\\');
                Data.Add(path,b); blobs.Add(source,Admission.GitBlob(b)); hashes.Add(source,Hash(b));
                pins.Add(new {role="source:"+source,path=path,sha256=Hash(b),id128=Id,size=b.Length,blob=Admission.GitBlob(b)});
            }
            Provenance=Node(new {schema=1,source_commit_claim=new string('a',40),runtime_authority=false,native_executed=false,source_sha256=hashes,helper_sha256=Hash(new byte[]{1,2})});
            foreach(string role in new[]{"controller","cargo","helper","libtest","selectors","provenance","workspace_lock","helper_lock"}) {
                string path=Root+"\\"+role+".bin";
                if(role=="helper") path=Root+@"\bin\cargo.exe";
                if(role=="libtest") path=Root+@"\image\libtest.exe";
                if(role=="workspace_lock") path=Root+@"\Cargo.lock";
                if(role=="helper_lock") path=Base+@"child\Cargo.lock";
                if(role=="selectors") path=Base+"selectors.json";
                byte[] b=Data.ContainsKey(path)?Data[path]:role=="provenance"?Bytes(Provenance):new byte[]{1,2};
                Data[path]=b;
                pins.Add(new {role=role,path=path,sha256=Hash(b),id128=Id,size=b.Length,blob="artifact"});
            }
            Manifest=Node(new {schema=1,runtime_authority=false,reviewed_commit=new string('a',40),root=Root,helper_directory=Root+@"\bin",temp=Root+@"\tmp",system_root=@"C:\Windows",
                target="x86_64-pc-windows-msvc",profile="dev",debug="0",features="default",toolchain="1.95.0",compiler_commit="59807616e1fa2540724bfbac14d7976d7e4a3860",cargo_version="1.95.0 (f2d3ce0bd7f24a49f8f72d9000448f8838c4e850)",pins=pins});
            Disposition=Node(new {schema=1,runtime_authority=false,reviewed_commit=new string('a',40),root=Root,manifest=new {sha256="",id128=Id,size=1},source_blobs=blobs});
            Seal();
        }
        internal System.Text.Json.Nodes.JsonNode Pin(string role) {
            foreach(var p in Manifest["pins"].AsArray()) if((string)p["role"]==role) return p;
            throw new Exception("fixture role");
        }
        internal void Seal() {
            byte[] p=Bytes(Provenance); Data[Root+@"\provenance.bin"]=p;
            Pin("provenance")["sha256"]=Hash(p); Pin("provenance")["size"]=p.Length;
            SealManifest();
        }
        internal void SealManifest() {
            byte[] m=Bytes(Manifest); Data[Base+"run-manifest.json"]=m;
            Disposition["manifest"]["sha256"]=Hash(m); Disposition["manifest"]["size"]=m.Length;
            SealDisposition();
        }
        internal void SealDisposition() { byte[] d=Bytes(Disposition); Data[Base+"run-disposition.json"]=d; Trust=Hash(d); }
        public byte[] ReadPinned(string path,string hash,int limit,string id) {
            Reads++;
            if(id!=Id || !Data.ContainsKey(path)) throw new InvalidOperationException("synthetic held read refused");
            byte[] b=Data[path];
            if(!Seen.Add(path) && Later.ContainsKey(path)) b=Later[path];
            if(path==BadRead) return new byte[]{9};
            if(b.Length>limit) throw new InvalidOperationException("synthetic bound refused");
            return (byte[])b.Clone();
        }
        sealed class Dir : IAdmissionDirectory {
            readonly Fixture f; readonly string path;
            internal Dir(Fixture f,string path) { this.f=f; this.path=path; }
            public HeldFiles.DirectoryEntry[] Scan() {
                f.Scans++; return new[]{new HeldFiles.DirectoryEntry(".",16,0,1,0),new HeldFiles.DirectoryEntry("..",16,0,2,0),new HeldFiles.DirectoryEntry(System.IO.Path.GetFileName(path),0,0,3,0)};
            }
            public byte[] ReadLeaf(Admission.Pin p) { return f.ReadPinned(p.path,p.sha256,p.size,p.id); }
        }
        public IAdmissionDirectory ExclusiveDirectory(string path) {
            if(path==Root+@"\bin") return new Dir(this,path+@"\cargo.exe");
            if(path==Root+@"\image") return new Dir(this,path+@"\libtest.exe");
            throw new InvalidOperationException("synthetic directory refused");
        }
        public void HoldEnvironment(string temp,string system) { Check(temp==Root+@"\tmp" && system==@"C:\Windows","environment"); }
        internal Admission.Facts Bind() { return Admission.Bind(this,Trust,Id,Root); }
    }
    static void Reject(string name,Action<Fixture> change,bool seal=true) {
        var f=new Fixture(); change(f); if(seal) f.Seal(); bool refused=false;
        try { f.Bind(); }
        catch(InvalidOperationException) { refused=true; }
        catch(System.Text.Json.JsonException) { refused=true; }
        catch(FormatException) { refused=true; }
        catch(KeyNotFoundException) { refused=true; }
        Check(refused,name);
    }
    internal static void Run() {
        // Independent Git oracle, not computed by the fixture's blob routine.
        Check(Admission.GitBlob(Encoding.ASCII.GetBytes("hello\n"))=="ce013625030ba8dba906f756967f9e9ca394464a","git framing");
        Check(HeldFiles.IdText(0x0123456789abcdef,0xfedcba9876543210)=="0123456789abcdeffedcba9876543210","qword order");
        var readFixture=new Fixture(); bool hashRefused=false;
        try { Admission.Read(readFixture,Root+@"\Cargo.lock",new string('f',64),7,Id); }
        catch(InvalidOperationException) { hashRefused=true; }
        Check(hashRefused,"held read hash");
        var f=new Fixture(); var facts=f.Bind();
        Check(facts.Commit==new string('a',40) && f.Scans==4 && f.Reads==Admission.Sources.Length+10,"complete held flow");
        var copy=facts.CopyPins(); copy[0].sha256="changed"; copy.Clear();
        Check(facts.CopyPins().Count==Admission.Sources.Length+8,"immutable facts");
        Reject("root hash",x=>x.Trust=new string('f',64),false);
        Reject("held manifest hash",x=>x.BadRead=Base+"run-manifest.json");
        Reject("held source hash",x=>x.BadRead=Root+@"\Cargo.lock");
        Reject("held helper hash",x=>x.BadRead=Root+@"\bin\cargo.exe");
        Reject("blob bytes",x=> { x.Pin("source:Cargo.lock")["blob"]=new string('b',40); x.Disposition["source_blobs"]["Cargo.lock"]=new string('b',40); });
        Reject("reviewed blob",x=>x.Disposition["source_blobs"]["Cargo.lock"]=new string('b',40));
        Reject("commit",x=>x.Manifest["reviewed_commit"]=new string('b',40));
        Reject("bad commit",x=>x.Disposition["reviewed_commit"]="main");
        Reject("provenance commit",x=>x.Provenance["source_commit_claim"]=new string('b',40));
        Reject("provenance source",x=>x.Provenance["source_sha256"]["Cargo.lock"]=new string('b',64));
        Reject("provenance helper",x=>x.Provenance["helper_sha256"]=new string('b',64));
        Reject("manifest authority",x=>x.Manifest["runtime_authority"]=true);
        Reject("disposition authority",x=>x.Disposition["runtime_authority"]=true);
        Reject("provenance authority",x=>x.Provenance["runtime_authority"]=true);
        Reject("native claim",x=>x.Provenance["native_executed"]=true);
        foreach(string role in new[]{"workspace_lock","helper_lock","selectors"}) {
            Reject("alias "+role,x=> { var p=x.Pin(role); string path=Root+"\\alias.bin"; x.Data[path]=x.Data[(string)p["path"]]; p["path"]=path; });
            Reject("alias id "+role,x=>x.Pin(role)["id128"]=new string('e',32));
        }
        Reject("lock bytes",x=> { var p=x.Pin("workspace_lock"); byte[] b={9}; p["sha256"]=Hash(b); p["size"]=1; x.Later[Root+@"\Cargo.lock"]=b; });
        Reject("source CR",x=> { byte[] b={13}; var p=x.Pin("source:Cargo.lock"); x.Data[Root+@"\Cargo.lock"]=b; p["sha256"]=Hash(b); p["size"]=1; p["blob"]=Admission.GitBlob(b); x.Disposition["source_blobs"]["Cargo.lock"]=Admission.GitBlob(b); });
        Reject("provenance limit",x=> { x.Pin("provenance")["size"]=65537; x.SealManifest(); },false);
        Reject("role alias",x=>x.Pin("controller")["path"]=x.Pin("cargo")["path"].DeepClone());
        foreach(string id in new[]{new string('0',32),new string('A',32),new string('a',31),new string('a',33)}) Reject("id format",x=>x.Pin("controller")["id128"]=id);
        Reject("unknown manifest",x=>x.Manifest["extra"]=0);
        Reject("unknown disposition",x=>x.Disposition["extra"]=0);
        Reject("unknown provenance",x=>x.Provenance["extra"]=0);
        Reject("unknown role",x=>x.Pin("controller")["role"]="unknown");
        Reject("duplicate role",x=>x.Pin("controller")["role"]="cargo");
        Reject("missing source blob",x=>x.Disposition["source_blobs"].AsObject().Remove("Cargo.lock"));
        Reject("unknown source hash",x=>x.Provenance["source_sha256"]["unknown"]=new string('b',64));
        Reject("manifest limit",x=> { x.Disposition["manifest"]["size"]=65537; x.SealDisposition(); },false);
        Reject("duplicate disposition key",x=> { byte[] b=Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(x.Data[Base+"run-disposition.json"]).Insert(1,"\"schema\":1,")); x.Data[Base+"run-disposition.json"]=b; x.Trust=Hash(b); },false);
        Reject("duplicate manifest key",x=> { byte[] b=Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(x.Data[Base+"run-manifest.json"]).Insert(1,"\"schema\":1,")); x.Data[Base+"run-manifest.json"]=b; x.Disposition["manifest"]["sha256"]=Hash(b); x.Disposition["manifest"]["size"]=b.Length; x.SealDisposition(); },false);
        Reject("duplicate provenance key",x=> { byte[] b=Encoding.UTF8.GetBytes(Encoding.UTF8.GetString(x.Data[Root+@"\provenance.bin"]).Insert(1,"\"schema\":1,")); x.Data[Root+@"\provenance.bin"]=b; x.Pin("provenance")["sha256"]=Hash(b); x.Pin("provenance")["size"]=b.Length; x.SealManifest(); },false);
        Reject("malformed disposition",x=> { byte[] b=Encoding.UTF8.GetBytes("{"); x.Data[Base+"run-disposition.json"]=b; x.Trust=Hash(b); },false);
        Reject("malformed manifest",x=> { byte[] b=Encoding.UTF8.GetBytes("{"); x.Data[Base+"run-manifest.json"]=b; x.Disposition["manifest"]["sha256"]=Hash(b); x.Disposition["manifest"]["size"]=1; byte[] d=Bytes(x.Disposition); x.Data[Base+"run-disposition.json"]=d; x.Trust=Hash(d); },false);
    }
}
