// Dormant native adapter. Compile-only in Gate 2; no test calls these imports.
using System;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using Microsoft.Win32.SafeHandles;

internal sealed class NativeJob : IDisposable {
    [StructLayout(LayoutKind.Sequential)] internal struct Security { internal int size; internal IntPtr descriptor; internal int inherit; }
    [StructLayout(LayoutKind.Sequential)] internal struct Startup {
        internal int size; internal IntPtr reserved,desktop,title; internal uint x,y,cx,cy,cols,rows,fill,flags;
        internal ushort show,reservedSize; internal IntPtr reservedBytes,input,output,error,attributes;
    }
    [StructLayout(LayoutKind.Sequential)] internal struct ProcessInfo { internal IntPtr process,thread; internal uint pid,tid; }
    [StructLayout(LayoutKind.Sequential)] struct BasicLimits {
        internal long processTime,jobTime; internal uint flags; internal UIntPtr min,max;
        internal uint active; internal UIntPtr affinity; internal uint priority,scheduling;
    }
    [StructLayout(LayoutKind.Sequential)] struct Limits {
        internal BasicLimits basic; internal ulong readOps,writeOps,otherOps,readBytes,writeBytes,otherBytes;
        internal UIntPtr processMemory,jobMemory,peakProcess,peakJob;
    }
    [StructLayout(LayoutKind.Sequential)] struct Accounting { internal long a,b,c,d; internal uint faults,total,active,terminated; }
    [DllImport("kernel32.dll", SetLastError=true)] static extern SafeFileHandle CreateJobObjectW(IntPtr security,IntPtr name);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetInformationJobObject(SafeFileHandle job,int kind,ref Limits limits,int size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool QueryInformationJobObject(SafeFileHandle job,int kind,out Accounting info,int size,IntPtr length);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateJobObject(SafeFileHandle job,uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool AssignProcessToJobObject(SafeFileHandle job,SafeFileHandle process);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool CreatePipe(out SafeFileHandle read,out SafeFileHandle write,ref Security security,uint size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool SetHandleInformation(SafeFileHandle handle,uint mask,uint flags);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool InitializeProcThreadAttributeList(IntPtr list,int count,uint flags,ref IntPtr size);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool UpdateProcThreadAttribute(IntPtr list,uint flags,UIntPtr attribute,IntPtr value,IntPtr size,IntPtr previous,IntPtr returned);
    [DllImport("kernel32.dll")] static extern void DeleteProcThreadAttributeList(IntPtr list);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode,SetLastError=true)] static extern bool CreateProcessW(string app,StringBuilder command,IntPtr ps,IntPtr ts,bool inherit,uint flags,byte[] env,string cwd,ref Startup startup,out ProcessInfo info);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint ResumeThread(SafeFileHandle thread);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool TerminateProcess(SafeFileHandle process,uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern uint WaitForSingleObject(SafeFileHandle process,uint timeout);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool GetExitCodeProcess(SafeFileHandle process,out uint code);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool PeekNamedPipe(SafeFileHandle pipe,IntPtr data,uint size,IntPtr read,out uint available,IntPtr left);
    [DllImport("kernel32.dll", SetLastError=true)] static extern bool ReadFile(SafeFileHandle pipe,byte[] bytes,uint size,out uint read,IntPtr overlapped);
    [DllImport("kernel32.dll", CharSet=CharSet.Unicode,SetLastError=true)] static extern SafeFileHandle CreateFileW(string path,uint access,uint share,ref Security security,uint creation,uint flags,IntPtr template);
    SafeFileHandle job,process,thread,telemetry;
    readonly System.Collections.Generic.List<SafeFileHandle> retired=new System.Collections.Generic.List<SafeFileHandle>();
    sealed class PipeReader : ReplayController.ITelemetryRead {
        readonly SafeFileHandle pipe;
        internal PipeReader(SafeFileHandle pipe) { this.pipe=pipe; }
        public bool Peek(out uint available,out bool eof) {
            eof=false;
            if(PeekNamedPipe(pipe,IntPtr.Zero,0,IntPtr.Zero,out available,IntPtr.Zero)) return true;
            eof=Marshal.GetLastWin32Error()==109;
            if(eof) available=0; // failed API output is not a byte count
            return eof;
        }
        public bool Read(byte[] bytes,int count,out uint read) {
            return ReadFile(pipe,bytes,(uint)count,out read,IntPtr.Zero);
        }
    }
    PipeReader reader;
    readonly byte[] buffer=new byte[4096];
    bool disposed;
    readonly object lifetime=new object();
    internal bool Disposed { get { return Volatile.Read(ref disposed); } }
    int cancelled;
    readonly ReplayController.Supervisor supervisor;
    void CheckCancelled() {
        if(Volatile.Read(ref cancelled)==0 && supervisor.ContinueOperation) return;
        // A late CreateProcess/Assign completion remains suspended. Job kill is
        // not a persistent admission fence: explicitly terminate the owned handle.
        Cancel(); throw new InvalidOperationException("late launch refused");
    }
    internal void Cancel() {
        Interlocked.Exchange(ref cancelled,1);
        lock(lifetime) {
            if(disposed) return; // closed kill-on-close owner, no outstanding launch
            TerminateJobObject(job,1); // always before owned unassigned cleanup
            var owned=Volatile.Read(ref process);
            if(owned!=null) TerminateProcess(owned,1);
        }
    }
    internal NativeJob(ReplayController.Supervisor supervisor) {
        this.supervisor=supervisor;
        job=CreateJobObjectW(IntPtr.Zero,IntPtr.Zero);
        var limits=new Limits { basic=new BasicLimits { flags=0x2000 } }; // kill on close; never breakaway
        if(job.IsInvalid || !SetInformationJobObject(job,9,ref limits,Marshal.SizeOf<Limits>())) { job.Dispose(); throw new InvalidOperationException("containment refused"); }
    }
    internal uint Active() {
        Accounting info;
        if(!QueryInformationJobObject(job,1,out info,Marshal.SizeOf<Accounting>(),IntPtr.Zero)) throw new InvalidOperationException("job query refused");
        return info.active;
    }
    static void Pipe(out SafeFileHandle read,out SafeFileHandle write) {
        var s=new Security {size=Marshal.SizeOf<Security>(),inherit=1};
        if(!CreatePipe(out read,out write,ref s,4096)) {
            read?.Dispose(); write?.Dispose(); throw new InvalidOperationException("pipe refused");
        }
        if(read.IsInvalid || write.IsInvalid) {
            read.Dispose(); write.Dispose(); throw new InvalidOperationException("invalid pipe refused");
        }
        if(!SetHandleInformation(read,1,0)) { read.Dispose(); write.Dispose(); throw new InvalidOperationException("inheritance refused"); }
    }
    internal void Launch(string app,string cwd,string[] args,byte[] env) {
        if(Volatile.Read(ref cancelled)!=0 || disposed || process!=null || Active()!=0) throw new InvalidOperationException("session reuse refused");
        SafeFileHandle input=null,discard=null,telemetryWrite=null;
        IntPtr list=IntPtr.Zero, values=IntPtr.Zero; bool initialized=false;
        try {
            // NUL is the sink for BOTH raw harness streams. Neither is decoded.
            var security=new Security {size=Marshal.SizeOf<Security>(),inherit=1};
            input=CreateFileW("NUL",0x80000000,3,ref security,3,0,IntPtr.Zero);
            discard=CreateFileW("NUL",0x40000000,3,ref security,3,0,IntPtr.Zero);
            if(input.IsInvalid || discard.IsInvalid) throw new InvalidOperationException("null streams refused");
            Pipe(out telemetry,out telemetryWrite);
            reader=new PipeReader(telemetry);
            IntPtr size=IntPtr.Zero;
            InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
            if(size.ToInt64()<=0 || size.ToInt64()>65536) throw new InvalidOperationException("attribute size refused");
            list=Marshal.AllocHGlobal(size); values=Marshal.AllocHGlobal(3*IntPtr.Size);
            if(!InitializeProcThreadAttributeList(list,1,0,ref size)) throw new InvalidOperationException("attribute list refused");
            initialized=true;
            Marshal.Copy(new[]{input.DangerousGetHandle(),discard.DangerousGetHandle(),telemetryWrite.DangerousGetHandle()},0,values,3);
            if(!UpdateProcThreadAttribute(list,0,(UIntPtr)0x20002,values,(IntPtr)(3*IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) throw new InvalidOperationException("handle list refused");
            var startup=new Startup {size=Marshal.SizeOf<Startup>(),flags=0x100,input=input.DangerousGetHandle(),output=discard.DangerousGetHandle(),error=discard.DangerousGetHandle(),attributes=list};
            var command=new StringBuilder(ReplayController.LaunchSpec.Quote(app));
            foreach(string arg in args) command.Append(' ').Append(ReplayController.LaunchSpec.Quote(arg));
            // Exactly three unique handles: NUL input, shared NUL output/error,
            // dedicated telemetry writer. Reader and Job are never inherited.
            // No argv/env slot conveys this value today. A future reviewed wrapper
            // must take ownership and clear INHERIT before any fixture child.
            RequireTelemetryBinding();
            CheckCancelled();
            ProcessInfo info;
            if(!CreateProcessW(app,command,IntPtr.Zero,IntPtr.Zero,true,0x80404,env,cwd,ref startup,out info)) throw new InvalidOperationException("launch refused");
            thread=new SafeFileHandle(info.thread,true);
            Volatile.Write(ref process,new SafeFileHandle(info.process,true));
            if(!supervisor.Created()) { Cancel(); throw new InvalidOperationException("late create refused"); }
            CheckCancelled();
            // Child remains suspended on nested-Job incompatibility. Explicitly
            // terminate the one owned unassigned process; never resume it.
            bool assigned=AssignProcessToJobObject(job,process);
            if(!supervisor.Assigned(assigned)) {
                TerminateJobObject(job,1); TerminateProcess(process,1);
                throw new InvalidOperationException("nested containment refused");
            }
            CheckCancelled();
            if(!supervisor.AuthorizeResume()) { Cancel(); throw new InvalidOperationException("resume authorization refused"); }
            if(ResumeThread(thread)!=1) throw new InvalidOperationException("resume refused");
        } catch { Cancel(); throw; }
        finally {
            if(initialized) DeleteProcThreadAttributeList(list);
            if(list!=IntPtr.Zero) Marshal.FreeHGlobal(list);
            if(values!=IntPtr.Zero) Marshal.FreeHGlobal(values);
            // Parent writer closes on success AND failure, before EOF is observed.
            input?.Dispose(); discard?.Dispose(); telemetryWrite?.Dispose();
        }
    }
    static void RequireTelemetryBinding() {
        throw new InvalidOperationException("telemetry launch binding grammar unreviewed");
    }
    internal bool Pump(ReplayController.FrameDecoder decoder,out bool eof) {
        return ReplayController.PumpTelemetry(reader,buffer,decoder,out eof);
    }
    internal bool Exited(out uint code) {
        code=0; uint wait=WaitForSingleObject(process,0);
        if(wait==258) return false;
        if(wait!=0 || !GetExitCodeProcess(process,out code)) throw new InvalidOperationException("exit query refused");
        return true;
    }
    internal void Next() {
        if(Volatile.Read(ref cancelled)!=0 || supervisor.Failed) throw new InvalidOperationException("failed reuse refused");
        // Retain every handle while the failure-only waiter may hold a snapshot.
        // At this point exit/EOF/active-zero were observed; no native close here.
        retired.Add(process); retired.Add(thread); retired.Add(telemetry);
        Interlocked.Exchange(ref process,null); thread=null; telemetry=null; reader=null;
    }
    internal bool KillAndConfirm(System.Diagnostics.Stopwatch clock,long until) {
        until=ReplayController.ConfirmationDeadline(until);
        // Termination precedes all unknown-cleanup outcomes, including query errors.
        Interlocked.Exchange(ref cancelled,1);
        bool killed=TerminateJobObject(job,1);
        if(process!=null) TerminateProcess(process,1); // owned suspended assign-failure child
        while(clock.ElapsedMilliseconds<until) {
            try { if(ReplayController.CleanupConfirmed(killed,Active(),process==null || WaitForSingleObject(process,0)==0,clock.ElapsedMilliseconds,until)) return true; }
            catch(InvalidOperationException) { }
            Thread.Sleep(1);
        }
        return false;
    }
    // If confirmation fails the session retains this owner; no remaining rows
    // or second launch is permitted. Kill-on-close is never cleared.
    public void Dispose() {
        // Only native lifetime/kill operations share this gate. Supervisor failure
        // latching never waits on it. A blocked close cannot confer success.
        lock(lifetime) {
            if(disposed) return;
            process?.Dispose(); thread?.Dispose(); telemetry?.Dispose();
            foreach(var handle in retired) handle?.Dispose();
            job.Dispose(); disposed=true;
        }
    }
}
