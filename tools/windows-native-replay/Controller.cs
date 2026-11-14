// Dormant controller source. Compile-only in Gate 2; never invoked by the driver.
// No PID/name kill. Every native handle is created here and closed here.
using System;
using System.Diagnostics;
using System.Runtime.InteropServices;
using System.Threading;

internal static class ReplayController
{
    const uint Suspended = 4, NoWindow = 0x08000000, WaitObject = 0;
    const uint KillOnClose = 0x2000;
    [StructLayout(LayoutKind.Sequential)] struct Startup {
        public uint cb; public IntPtr reserved, desktop, title;
        public uint x,y,xSize,ySize,xCount,yCount,fill,flags;
        public ushort show, reservedSize; public IntPtr reservedBytes, stdin, stdout, stderr;
    }
    [StructLayout(LayoutKind.Sequential)] struct ProcessInfo { public IntPtr process, thread; public uint pid, tid; }
    [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
        public long processTime,jobTime; public uint flags;
        public UIntPtr minWork,maxWork; public uint active; public UIntPtr affinity;
        public uint priority,scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] struct Io { public ulong ro,wo,oo,rb,wb,ob; }
    [StructLayout(LayoutKind.Sequential)] struct Limits {
        public BasicLimits basic; public Io io; public UIntPtr processMemory,jobMemory,peakProcess,peakJob;
    }
    [StructLayout(LayoutKind.Sequential)] struct Accounting {
        public long user,kernel,periodUser,periodKernel;
        public uint faults,total,active,terminated;
    }
    [DllImport("kernel32.dll", SetLastError=true)] static extern IntPtr CreateJobObjectW(IntPtr sa, IntPtr name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(IntPtr job,int cls,ref Limits info,uint len);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(IntPtr job,int cls,out Accounting info,uint len,IntPtr returned);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode, SetLastError=true)] static extern bool CreateProcessW(string app,System.Text.StringBuilder cmd,IntPtr pa,IntPtr ta,bool inherit,uint flags,IntPtr env,string cwd,ref Startup startup,out ProcessInfo process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(IntPtr job,IntPtr process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(IntPtr thread);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(IntPtr process,uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(IntPtr job,uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(IntPtr h,uint ms);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(IntPtr h,out uint code);
    [DllImport("kernel32.dll")] static extern bool CloseHandle(IntPtr h);

    // The future transport supplies sanitized, bounded numeric events through an
    // already-owned channel. Never parse Cargo stdout or publish raw output.
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
                if (running) return Reject();
                running=true; rowStart=elapsedMs; return true;
            }
            if (!running || elapsedMs-rowStart>=10000 || e.durationMs<0 || e.durationMs>10000 || e.clean!=1
                || e.consumed<0 || e.consumed>1 || e.creates<0 || e.creates>1 || e.resumes<0 || e.resumes>e.creates
                || e.stdout<0 || e.stdout>1 || e.stderr<0 || e.stderr>1) return Reject();
            // Kernel first, then 28 failures, four 109s, two controls.
            int expectedCreate=e.index==0 || (e.index>=1 && e.index<=14) ? 0:1;
            int expectedResume=e.index>=19 ? 1:0;
            int expectedConsumed=e.index>=1 && e.index<=32 ? 1:0;
            if(e.creates!=expectedCreate || e.resumes!=expectedResume || e.consumed!=expectedConsumed) return Reject();
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
    // Compile-only ownership implementation. Future reviewed driver must bind
    // exactly two literal Cargo commands, hash/path pins and nonblocking transport.
    // This method has no caller in Gate 2. No alternate uncontained fallback.
    internal static bool RunPinned(string executable,string command,string cwd,
        Func<Event?> tryReadOwnedEvent, Budget budget, Stopwatch global)
    {
        IntPtr job=IntPtr.Zero; ProcessInfo pi=new ProcessInfo();
        bool assigned=false, confirmed=false;
        try {
            job=CreateJobObjectW(IntPtr.Zero,IntPtr.Zero);
            if(job==IntPtr.Zero) return false;
            Limits limits=new Limits(); limits.basic.flags=KillOnClose;
            if(!SetInformationJobObject(job,9,ref limits,(uint)Marshal.SizeOf(typeof(Limits)))) return false;
            Startup si=new Startup(); si.cb=(uint)Marshal.SizeOf(typeof(Startup));
            // No inherited arbitrary handles and no BREAKAWAY flag. The Job
            // membership propagates to harness descendants including suspended S.
            if(!CreateProcessW(executable,new System.Text.StringBuilder(command),IntPtr.Zero,IntPtr.Zero,false,
                Suspended|NoWindow,IntPtr.Zero,cwd,ref si,out pi)) return false;
            if(!AssignProcessToJobObject(job,pi.process)) return false;
            assigned=true;
            if(ResumeThread(pi.thread)==UInt32.MaxValue) return false;
            // Waiting for the first row is bounded too; it cannot consume the
            // 360-second envelope without starting a per-row clock.
            long activity=global.ElapsedMilliseconds;
            while(true) {
                long now=global.ElapsedMilliseconds;
                if(budget.Expired(now) || now-activity>=10000) return false;
                Event? item=tryReadOwnedEvent(); // future transport MUST be nonblocking
                if(item.HasValue) {
                    if(!budget.Accept(item.Value,now)) return false;
                    activity=now;
                }
                if(WaitForSingleObject(pi.process,0)==WaitObject) {
                    uint code;
                    if(!GetExitCodeProcess(pi.process,out code) || code!=0) return false;
                    break;
                }
                Thread.Sleep(1);
            }
            // Only an actual query on our still-owned Job proves no descendants.
            Accounting a;
            confirmed=QueryInformationJobObject(job,1,out a,(uint)Marshal.SizeOf(typeof(Accounting)),IntPtr.Zero) && a.active==0;
            return confirmed;
        }
        finally {
            if(!confirmed) {
                if(pi.process!=IntPtr.Zero && !assigned) TerminateProcess(pi.process,1);
                if(job!=IntPtr.Zero) TerminateJobObject(job,1);
                // Unknown cleanup remains false: caller must stop, prohibit reuse,
                // and retain containment until authorized environment retirement.
            }
            if(pi.thread!=IntPtr.Zero) CloseHandle(pi.thread);
            if(pi.process!=IntPtr.Zero) CloseHandle(pi.process);
            if(job!=IntPtr.Zero) CloseHandle(job); // kill-on-close backstop
        }
    }
}

// Pure managed checks: no call to RunPinned, clock, process or Win32 API.
public static class PureControllerChecks {
    public static void Run() {
        var budget=new ReplayController.Budget();
        for(int i=0;i<35;i++) {
            if(!budget.Accept(new ReplayController.Event(i,true),i*1000)) throw new Exception("start");
            int c=i==0 || i<=14 ? 0:1, r=i>=19 ? 1:0, n=i>=1 && i<=32 ? 1:0;
            if(!budget.Accept(new ReplayController.Event(i,false,n,c,r),i*1000+1)) throw new Exception("end");
        }
        if(!budget.Complete || budget.children!=20 || budget.resumes!=16) throw new Exception("totals");
        var bad=new ReplayController.Budget();
        if(bad.Complete || bad.Accept(new ReplayController.Event(1,true),0)) throw new Exception("zero/order");
        if(bad.Accept(new ReplayController.Event(0,true),1)) throw new Exception("reuse after failure");
        bad=new ReplayController.Budget();
        bad.Accept(new ReplayController.Event(0,true),0);
        if(!bad.Expired(10000) || !bad.Expired(360000)) throw new Exception("deadline");
        if(bad.Accept(new ReplayController.Event(0,false,0,0,0,0),1)) throw new Exception("unknown cleanup");
        bad=new ReplayController.Budget();
        bad.Accept(new ReplayController.Event(0,true),0);
        if(bad.Accept(new ReplayController.Event(0,false,0,1),1)) throw new Exception("extra child");
    }
}
