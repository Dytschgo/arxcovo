// Gate 2 prepared partial. NO native launcher exists in this revision.
// Pure contracts below cannot grant authority or create a process.
using System;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Diagnostics;
using System.Threading;

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
    // A future containment adapter must own this session across BOTH filters.
    // No caller stopwatch or clock callback is accepted by production polling.
    internal sealed class OwnedSession {
        readonly Stopwatch clock=Stopwatch.StartNew();
        readonly MonitorState state=new MonitorState();
        internal readonly RowChannel channel=new RowChannel();
        internal bool Poll() { return state.Poll(channel,clock.ElapsedMilliseconds); }
        internal bool Success(bool exited,uint code,uint active) { return state.Success(channel,exited,code,active); }
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
        internal static bool Valid(string app,string cwd,string helper,string temp,string systemRoot,string[] args,IDictionary<string,string> env) {
            if(!Absolute(app) || !Absolute(cwd) || !Absolute(helper) || !Absolute(temp) || !Absolute(systemRoot)) return false;
            if(!app.EndsWith("\\cargo.exe",StringComparison.OrdinalIgnoreCase)) return false;
            if(args==null || args.Length!=12) return false;
            string[] fixedArgs={"test","-p","arxd","--lib","--locked","--offline","--target","x86_64-pc-windows-msvc",null,"--","--exact","--test-threads=1"};
            // Profile is an immutable build pin, not an ambient environment variable.
            if(args[8]!=Kernel && args[8]!=Matrix) return false;
            for(int i=0;i<12;i++) if(i!=8 && args[i]!=fixedArgs[i]) return false;
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
    public static void Run() {
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
        string[] args={"test","-p","arxd","--lib","--locked","--offline","--target","x86_64-pc-windows-msvc",ReplayController.LaunchSpec.Kernel,"--","--exact","--test-threads=1"};
        Check(ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"launch grammar");
        args[8]="arbitrary"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"filter reject"); args[8]=ReplayController.LaunchSpec.Kernel;
        env["OWNER_SECRET"]="synthetic"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"environment reject"); env.Remove("OWNER_SECRET");
        env["PATH"]+=@";C:\other"; Check(!ReplayController.LaunchSpec.Valid(app,cwd,helper,temp,root,args,env),"path reject");
        foreach(string path in new[]{"cargo.exe",@"C:\a\..\cargo.exe",@"\\server\cargo.exe",@"C:\a:stream"}) Check(!ReplayController.LaunchSpec.Absolute(path),"absolute reject");
    }
}
