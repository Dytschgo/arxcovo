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
    SafeFileHandle job,process,thread,output,error;
    readonly byte[] buffer=new byte[4096];
    bool disposed;
    internal NativeJob() {
        job=CreateJobObjectW(IntPtr.Zero,IntPtr.Zero);
        var limits=new Limits { basic=new BasicLimits { flags=0x2000 } }; // kill on close; never breakaway
        if(job.IsInvalid || !SetInformationJobObject(job,9,ref limits,Marshal.SizeOf<Limits>())) { job.Dispose(); throw new InvalidOperationException("containment refused"); }
    }
    internal uint Active() {
        Accounting info;
        if(!QueryInformationJobObject(job,1,out info,Marshal.SizeOf<Accounting>(),IntPtr.Zero)) throw new InvalidOperationException("job query refused");
        return info.active;
    }
    static void Pipe(out SafeFileHandle read,out SafeFileHandle write,bool inheritRead) {
        var s=new Security {size=Marshal.SizeOf<Security>(),inherit=1};
        if(!CreatePipe(out read,out write,ref s,4096)) throw new InvalidOperationException("pipe refused");
        if(!SetHandleInformation(inheritRead?write:read,1,0)) { read.Dispose(); write.Dispose(); throw new InvalidOperationException("inheritance refused"); }
    }
    internal void Launch(string app,string cwd,string[] args,byte[] env) {
        if(disposed || process!=null || Active()!=0) throw new InvalidOperationException("session reuse refused");
        SafeFileHandle input=null,inputWrite=null,outWrite=null,errWrite=null;
        IntPtr list=IntPtr.Zero, values=IntPtr.Zero; bool initialized=false;
        try {
            Pipe(out input,out inputWrite,true); Pipe(out output,out outWrite,false); Pipe(out error,out errWrite,false);
            IntPtr size=IntPtr.Zero;
            InitializeProcThreadAttributeList(IntPtr.Zero,1,0,ref size);
            if(size.ToInt64()<=0 || size.ToInt64()>65536) throw new InvalidOperationException("attribute size refused");
            list=Marshal.AllocHGlobal(size); values=Marshal.AllocHGlobal(3*IntPtr.Size);
            if(!InitializeProcThreadAttributeList(list,1,0,ref size)) throw new InvalidOperationException("attribute list refused");
            initialized=true;
            Marshal.Copy(new[]{input.DangerousGetHandle(),outWrite.DangerousGetHandle(),errWrite.DangerousGetHandle()},0,values,3);
            if(!UpdateProcThreadAttribute(list,0,(UIntPtr)0x20002,values,(IntPtr)(3*IntPtr.Size),IntPtr.Zero,IntPtr.Zero)) throw new InvalidOperationException("handle list refused");
            var startup=new Startup {size=Marshal.SizeOf<Startup>(),flags=0x100,input=input.DangerousGetHandle(),output=outWrite.DangerousGetHandle(),error=errWrite.DangerousGetHandle(),attributes=list};
            var command=new StringBuilder(ReplayController.LaunchSpec.Quote(app));
            foreach(string arg in args) command.Append(' ').Append(ReplayController.LaunchSpec.Quote(arg));
            ProcessInfo info;
            if(!CreateProcessW(app,command,IntPtr.Zero,IntPtr.Zero,true,0x80404,env,cwd,ref startup,out info)) throw new InvalidOperationException("launch refused");
            process=new SafeFileHandle(info.process,true); thread=new SafeFileHandle(info.thread,true);
            // Child remains suspended on nested-Job incompatibility. Explicitly
            // terminate the one owned unassigned process; never resume it.
            if(!AssignProcessToJobObject(job,process)) {
                TerminateJobObject(job,1); TerminateProcess(process,1);
                throw new InvalidOperationException("nested containment refused");
            }
            if(ResumeThread(thread)!=1) throw new InvalidOperationException("resume refused");
        } catch { TerminateJobObject(job,1); throw; }
        finally {
            if(initialized) DeleteProcThreadAttributeList(list);
            if(list!=IntPtr.Zero) Marshal.FreeHGlobal(list);
            if(values!=IntPtr.Zero) Marshal.FreeHGlobal(values);
            input?.Dispose(); inputWrite?.Dispose(); outWrite?.Dispose(); errWrite?.Dispose();
        }
    }
    bool ReadPipe(SafeFileHandle pipe,ReplayController.FrameDecoder decoder,out bool eof) {
        eof=false; uint available;
        if(!PeekNamedPipe(pipe,IntPtr.Zero,0,IntPtr.Zero,out available,IntPtr.Zero)) {
            if(Marshal.GetLastWin32Error()==109) { eof=true; return true; }
            return false;
        }
        if(available==0) return true;
        uint read;
        if(!ReadFile(pipe,buffer,Math.Min(available,4096),out read,IntPtr.Zero) || read==0) return false;
        return decoder==null || decoder.Feed(buffer,(int)read); // raw stdout discarded
    }
    internal bool Pump(ReplayController.FrameDecoder decoder,out bool eof) {
        bool ignored;
        return ReadPipe(output,null,out ignored) & ReadPipe(error,decoder,out eof);
    }
    internal bool Exited(out uint code) {
        code=0; uint wait=WaitForSingleObject(process,0);
        if(wait==258) return false;
        if(wait!=0 || !GetExitCodeProcess(process,out code)) throw new InvalidOperationException("exit query refused");
        return true;
    }
    internal void Next() { process.Dispose(); thread.Dispose(); output.Dispose(); error.Dispose(); process=null; thread=null; output=null; error=null; }
    internal bool KillAndConfirm(System.Diagnostics.Stopwatch clock) {
        // Termination precedes all unknown-cleanup outcomes, including query errors.
        bool killed=TerminateJobObject(job,1);
        if(process!=null) TerminateProcess(process,1); // owned suspended assign-failure child
        long until=Math.Min(360000,clock.ElapsedMilliseconds+2000);
        do {
            try { if(ReplayController.CleanupConfirmed(killed,Active(),process==null || WaitForSingleObject(process,0)==0,clock.ElapsedMilliseconds,until)) return true; }
            catch(InvalidOperationException) { }
            Thread.Sleep(1);
        } while(clock.ElapsedMilliseconds<until);
        return false;
    }
    // If confirmation fails the session retains this owner; no remaining rows
    // or second launch is permitted. Kill-on-close is never cleared.
    public void Dispose() {
        if(disposed) return; disposed=true;
        process?.Dispose(); thread?.Dispose(); output?.Dispose(); error?.Dispose(); job.Dispose();
    }
}
