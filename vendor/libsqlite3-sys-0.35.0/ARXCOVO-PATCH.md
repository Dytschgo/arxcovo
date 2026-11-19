# Arxcovo SQLite bound identity patch v1

Upstream libsqlite3-sys 0.35.0, links sqlite3, bundled SQLite 3.50.2.
Archive SHA256: 133c182a6a2c87864fe97778797e46c7e999672690dc9fa3ee8e241aa4a9c13f.
SQLite source ID: 2025-06-28 14:00:48 2af157d77fb1304a74176eaee7fbc7c7e932d946bf25325e9c26c91db19e3079.
The crate retains its MIT license and SQLite public-domain notices. No relicense.
All 28 upstream archive members are retained; only the four inventoried files differ.
ARXCOVO-PINS.json records original and final bytes/hashes. It is change detection,
not an effective-link attestation. scripts/check-bound-sqlite.py verifies only;
updates require independent exact-diff review and target qualification.

ABI: operation 0x41525831, 20 native u32 words, size80/align4; header magic
0x41525849/version1/bytes80; fields and offsets are asserted in C and Rust.
Status 0 success,1 unsupported,2 syscall,3 range; success written last.
The Unix case fstats the actual live VFS descriptor, never returns it. Native
VFS/method pointer checks exclude aliases/wrappers. Windows borrows opcode29 only.
Exports sqlite3_arxcovo_bound_abi_v1 and sqlite3_arxcovo_qualify_main_v1.
Build guard runs first, permits exact seven-feature baseline and refuses overrides.
Upstream default compiler flags, SQL policy and optional source files are preserved.
Toolchain trust is required: malicious compiler/linker/config/attestation forgery
is outside this boundary. Effective Cargo configs and resolved path/features must
also pass the verifier. No production activation or store construction in slice1.
Unix Linux/macOS x86_64/aarch64 and Windows x64 MSVC only are prospective qualified
platforms. Windows NTFS evidence does not qualify other filesystems.

Exact maintained upstream hunks follow (no executable patch applicator):

```diff
--- upstream/build.rs
+++ patched/build.rs
@@ -1,3 +1,50 @@
+// ARXCOVO: this fork supports exactly the reviewed bundled configuration.
+fn arxcovo_build_guard() {
+    use std::collections::BTreeSet;
+    const FEATURES: &[&str] = &["BUNDLED", "BUNDLED_BINDINGS", "CC", "DEFAULT",
+        "MIN_SQLITE_VERSION_3_14_0", "PKG_CONFIG", "VCPKG"];
+    let actual: BTreeSet<_> = env::vars_os().filter_map(|(k,_)| {
+        k.to_str()?.strip_prefix("CARGO_FEATURE_").map(str::to_owned)
+    }).collect();
+    let expected: BTreeSet<_> = FEATURES.iter().map(|s| s.to_string()).collect();
+    assert_eq!(actual, expected, "ARXCOVO requires exact bundled feature baseline");
+    let target = env::var("TARGET").expect("TARGET");
+    let host = env::var("HOST").expect("HOST");
+    let overrides = ["LIBSQLITE3_SYS_USE_PKG_CONFIG", "SQLITE3_LIB_DIR", "SQLITE3_INCLUDE_DIR",
+        "SQLITE3_STATIC", "SQLCIPHER_LIB_DIR", "SQLCIPHER_INCLUDE_DIR", "SQLCIPHER_STATIC",
+        "LIBSQLITE3_SYS_BUNDLING", "LIBSQLITE3_FLAGS", "SQLITE_MAX_VARIABLE_NUMBER",
+        "SQLITE_MAX_EXPR_DEPTH", "SQLITE_MAX_COLUMN", "CFLAGS", "CPPFLAGS", "CC_FORCE_DISABLE",
+        "CC_NO_DEFAULTS", "RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS", "LD_PRELOAD"];
+    for name in overrides {
+        println!("cargo:rerun-if-env-changed={name}");
+        for triple in [&target, &host] {
+            for t in [triple.to_string(), triple.replace('-', "_")] {
+                println!("cargo:rerun-if-env-changed={name}_{t}");
+                println!("cargo:rerun-if-env-changed={t}_{name}");
+            }
+        }
+        for prefix in ["HOST", "TARGET"] {
+            println!("cargo:rerun-if-env-changed={prefix}_{name}");
+        }
+    }
+    for (key, value) in env::vars_os() {
+        let key = key.to_str().expect("non Unicode environment key").to_ascii_uppercase();
+        let forbidden = key.contains("SQLITE") || key.contains("SQLCIPHER")
+            || key.contains("CFLAGS") || key.contains("CPPFLAGS")
+            || key.contains("CC_FORCE_DISABLE") || key.contains("CC_NO_DEFAULTS")
+            || key.starts_with("DYLD_") || key=="LD_PRELOAD"
+            || key=="RUSTFLAGS" || key=="CARGO_ENCODED_RUSTFLAGS"
+            || (key.starts_with("CARGO_TARGET_") &&
+                (key.ends_with("_RUSTFLAGS") || key.ends_with("_RUSTC_LINKER")));
+        // Cargo itself supplies these feature variables and package metadata.
+        let cargo_metadata = key.starts_with("CARGO_FEATURE_") || key=="CARGO_PKG_NAME"
+            || key=="CARGO_MANIFEST_DIR" || key=="CARGO_MANIFEST_PATH";
+        let empty_cargo_flags = key=="CARGO_ENCODED_RUSTFLAGS" && value.is_empty();
+        assert!(!forbidden || cargo_metadata || empty_cargo_flags,
+            "ARXCOVO unsupported build override: {key}");
+    }
+}
+
 use std::env;
 use std::path::Path;
 
@@ -40,6 +87,7 @@
 }
 
 fn main() {
+    arxcovo_build_guard();
     let out_dir = env::var("OUT_DIR").unwrap();
     let out_path = Path::new(&out_dir).join("bindgen.rs");
     if cfg!(feature = "in_gecko") {
--- upstream/sqlite3/bindgen_bundled_version.rs
+++ patched/sqlite3/bindgen_bundled_version.rs
@@ -3518,3 +3518,19 @@
         ) -> ::std::os::raw::c_int,
     >,
 }
+
+// ARXCOVO private actual-open ABI v1; maintained without regenerating bindings.
+pub const ARX_FCNTL_IDENTITY_V1: ::std::os::raw::c_int = 0x41525831;
+#[repr(C)]
+#[derive(Debug, Copy, Clone)]
+pub struct ArxIdentityV1 {
+    pub magic: u32, pub abi_version: u32, pub struct_bytes: u32,
+    pub status: u32, pub os_errno: u32, pub vfs_kind: u32,
+    pub dev_lo: u32, pub dev_hi: u32, pub ino_lo: u32, pub ino_hi: u32,
+    pub nlink_lo: u32, pub nlink_hi: u32, pub mode: u32, pub flags: u32,
+    pub reserved: [u32; 6],
+}
+unsafe extern "C" {
+    pub fn sqlite3_arxcovo_bound_abi_v1() -> ::std::os::raw::c_int;
+    pub fn sqlite3_arxcovo_qualify_main_v1(db: *mut sqlite3) -> ::std::os::raw::c_int;
+}
--- upstream/sqlite3/sqlite3.c
+++ patched/sqlite3/sqlite3.c
@@ -1548,6 +1548,36 @@
 #define SQLITE_FCNTL_LAST_ERRNO              4
 #define SQLITE_FCNTL_SIZE_HINT               5
 #define SQLITE_FCNTL_CHUNK_SIZE              6
+/* ARXCOVO private actual-open ABI v1. SQLite remains public domain. */
+#include <stdint.h>
+#include <stddef.h>
+#define ARX_FCNTL_IDENTITY_V1 0x41525831
+typedef struct ArxIdentityV1 {
+  uint32_t magic, abi_version, struct_bytes, status, os_errno, vfs_kind;
+  uint32_t dev_lo, dev_hi, ino_lo, ino_hi, nlink_lo, nlink_hi, mode, flags;
+  uint32_t reserved[6];
+} ArxIdentityV1;
+typedef char arx_offset_magic[(offsetof(ArxIdentityV1,magic)==0)?1:-1];
+typedef char arx_offset_abi_version[(offsetof(ArxIdentityV1,abi_version)==4)?1:-1];
+typedef char arx_offset_struct_bytes[(offsetof(ArxIdentityV1,struct_bytes)==8)?1:-1];
+typedef char arx_offset_status[(offsetof(ArxIdentityV1,status)==12)?1:-1];
+typedef char arx_offset_os_errno[(offsetof(ArxIdentityV1,os_errno)==16)?1:-1];
+typedef char arx_offset_vfs_kind[(offsetof(ArxIdentityV1,vfs_kind)==20)?1:-1];
+typedef char arx_offset_dev_lo[(offsetof(ArxIdentityV1,dev_lo)==24)?1:-1];
+typedef char arx_offset_dev_hi[(offsetof(ArxIdentityV1,dev_hi)==28)?1:-1];
+typedef char arx_offset_ino_lo[(offsetof(ArxIdentityV1,ino_lo)==32)?1:-1];
+typedef char arx_offset_ino_hi[(offsetof(ArxIdentityV1,ino_hi)==36)?1:-1];
+typedef char arx_offset_nlink_lo[(offsetof(ArxIdentityV1,nlink_lo)==40)?1:-1];
+typedef char arx_offset_nlink_hi[(offsetof(ArxIdentityV1,nlink_hi)==44)?1:-1];
+typedef char arx_offset_mode[(offsetof(ArxIdentityV1,mode)==48)?1:-1];
+typedef char arx_offset_flags[(offsetof(ArxIdentityV1,flags)==52)?1:-1];
+typedef char arx_offset_reserved[(offsetof(ArxIdentityV1,reserved)==56)?1:-1];
+typedef char arx_size[(sizeof(ArxIdentityV1)==80)?1:-1];
+struct ArxAlignmentV1 { char c; ArxIdentityV1 v; };
+typedef char arx_align[(offsetof(struct ArxAlignmentV1,v)==4)?1:-1];
+SQLITE_API int sqlite3_arxcovo_bound_abi_v1(void);
+SQLITE_API int sqlite3_arxcovo_qualify_main_v1(sqlite3*);
+
 #define SQLITE_FCNTL_FILE_POINTER            7
 #define SQLITE_FCNTL_SYNC_OMITTED            8
 #define SQLITE_FCNTL_WIN32_AV_RETRY          9
@@ -42819,9 +42849,52 @@
 /*
 ** Information and control of an open file handle.
 */
+static sqlite3_vfs *arxUnixVfs;
+static int arxUnixNative(sqlite3_file*, sqlite3_vfs*);
+static int arxUnixIdentity(unixFile *pFile, void *pArg){
+  ArxIdentityV1 *p = (ArxIdentityV1*)pArg;
+  struct stat st;
+  if( p==0 ) return SQLITE_MISUSE;
+  if( p->magic!=0x41525849 || p->abi_version!=1 || p->struct_bytes!=80 )
+    return SQLITE_MISMATCH;
+  memset(((char*)p)+12, 0, 68);
+  p->status = 1;
+  if( !arxUnixNative((sqlite3_file*)pFile, pFile->pVfs) )
+    return SQLITE_NOTFOUND;
+  if( pFile->h<0 || fstat(pFile->h, &st)!=0 ){
+    int saved = errno;
+    p->status = 2;
+    if( saved>0 && (sqlite3_uint64)saved<=0x7fffffff ) p->os_errno=(uint32_t)saved;
+    return SQLITE_IOERR_FSTAT;
+  }
+  /* Signed native values are checked before conversion; no sign extension.
+  ** sizeof also refuses future wider native representations. */
+#define ARX_NONNEG(x) (((x)>0) || ((x)==0))
+  if( sizeof(st.st_dev)>8 || sizeof(st.st_ino)>8 || sizeof(st.st_nlink)>8
+   || sizeof(st.st_mode)>4 || !ARX_NONNEG(st.st_dev) || !ARX_NONNEG(st.st_ino)
+   || !ARX_NONNEG(st.st_nlink) || !ARX_NONNEG(st.st_mode) ){
+    p->status = 3;
+    return SQLITE_RANGE;
+  }
+#undef ARX_NONNEG
+#define ARX_WORDS(name,value) p->name##_lo=(uint32_t)(value); p->name##_hi=(uint32_t)(((sqlite3_uint64)(value))>>32)
+  ARX_WORDS(dev,st.st_dev);
+  ARX_WORDS(ino,st.st_ino);
+  ARX_WORDS(nlink,st.st_nlink);
+#undef ARX_WORDS
+  p->mode = (uint32_t)st.st_mode;
+#if defined(__APPLE__)
+  p->vfs_kind = 2;
+#else
+  p->vfs_kind = 1;
+#endif
+  p->status = 0;
+  return SQLITE_OK;
+}
 static int unixFileControl(sqlite3_file *id, int op, void *pArg){
   unixFile *pFile = (unixFile*)id;
   switch( op ){
+    case ARX_FCNTL_IDENTITY_V1: return arxUnixIdentity(pFile,pArg);
 #if defined(__linux__) && defined(SQLITE_ENABLE_BATCH_ATOMIC_WRITE)
     case SQLITE_FCNTL_BEGIN_ATOMIC_WRITE: {
       int rc = osIoctl(pFile->h, F2FS_IOC_START_ATOMIC_WRITE);
@@ -44639,6 +44712,15 @@
 /*
 ** Initialize the contents of the unixFile structure pointed to by pId.
 */
+static int arxUnixNative(sqlite3_file *file, sqlite3_vfs *vfs){
+#if (defined(__linux__) || defined(__APPLE__)) && (defined(__x86_64__) || defined(__aarch64__))
+  return file && arxUnixVfs && vfs==arxUnixVfs
+      && file->pMethods==&posixIoMethods;
+#else
+  return 0;
+#endif
+}
+
 static int fillInUnixFile(
   sqlite3_vfs *pVfs,      /* Pointer to vfs object */
   int h,                  /* Open file descriptor of file being opened */
@@ -47073,6 +47155,7 @@
   ** correctly.  See ticket [bb3a86e890c8e96ab] */
   assert( ArraySize(aSyscall)==29 );
 
+  arxUnixVfs = &aVfs[0];
   /* Register all VFSes defined in the aVfs[] array */
   for(i=0; i<(sizeof(aVfs)/sizeof(sqlite3_vfs)); i++){
 #ifdef SQLITE_DEFAULT_UNIX_VFS
@@ -52210,6 +52293,14 @@
   winUnfetch                      /* xUnfetch */
 };
 
+static sqlite3_vfs *arxWinVfs;
+static int arxWinNative(sqlite3_file *file, sqlite3_vfs *vfs){
+#if defined(_WIN64) && defined(_M_X64) && defined(_MSC_VER)
+  return file && arxWinVfs && vfs==arxWinVfs && file->pMethods==&winIoMethod;
+#else
+  return 0;
+#endif
+}
 static winVfsAppData winAppData = {
   &winIoMethod,       /* pMethod */
   0,                  /* pAppData */
@@ -53773,6 +53864,7 @@
   assert( winSysInfo.dwAllocationGranularity>0 );
   assert( winSysInfo.dwPageSize>0 );
 
+  arxWinVfs = &winVfs;
   sqlite3_vfs_register(&winVfs, 1);
 
 #if defined(SQLITE_WIN32_HAS_WIDE)
@@ -53810,6 +53902,25 @@
 #endif /* SQLITE_OS_WIN */
 
 /************** End of os_win.c **********************************************/
+SQLITE_API int sqlite3_arxcovo_bound_abi_v1(void){ return 1; }
+SQLITE_API int sqlite3_arxcovo_qualify_main_v1(sqlite3 *db){
+  sqlite3_file *file = 0;
+  sqlite3_vfs *vfs = 0;
+  int rc;
+  if( db==0 ) return SQLITE_MISUSE;
+  rc = sqlite3_file_control(db,"main",SQLITE_FCNTL_FILE_POINTER,&file);
+  if( rc!=SQLITE_OK ) return rc;
+  rc = sqlite3_file_control(db,"main",SQLITE_FCNTL_VFS_POINTER,&vfs);
+  if( rc!=SQLITE_OK ) return rc;
+#if SQLITE_OS_UNIX
+  return arxUnixNative(file,vfs) ? SQLITE_OK : SQLITE_NOTFOUND;
+#elif SQLITE_OS_WIN
+  return arxWinNative(file,vfs) ? SQLITE_OK : SQLITE_NOTFOUND;
+#else
+  return SQLITE_NOTFOUND;
+#endif
+}
+
 /************** Begin file memdb.c *******************************************/
 /*
 ** 2016-09-07
--- upstream/sqlite3/sqlite3.h
+++ patched/sqlite3/sqlite3.h
@@ -1229,6 +1229,36 @@
 #define SQLITE_FCNTL_LAST_ERRNO              4
 #define SQLITE_FCNTL_SIZE_HINT               5
 #define SQLITE_FCNTL_CHUNK_SIZE              6
+/* ARXCOVO private actual-open ABI v1. SQLite remains public domain. */
+#include <stdint.h>
+#include <stddef.h>
+#define ARX_FCNTL_IDENTITY_V1 0x41525831
+typedef struct ArxIdentityV1 {
+  uint32_t magic, abi_version, struct_bytes, status, os_errno, vfs_kind;
+  uint32_t dev_lo, dev_hi, ino_lo, ino_hi, nlink_lo, nlink_hi, mode, flags;
+  uint32_t reserved[6];
+} ArxIdentityV1;
+typedef char arx_offset_magic[(offsetof(ArxIdentityV1,magic)==0)?1:-1];
+typedef char arx_offset_abi_version[(offsetof(ArxIdentityV1,abi_version)==4)?1:-1];
+typedef char arx_offset_struct_bytes[(offsetof(ArxIdentityV1,struct_bytes)==8)?1:-1];
+typedef char arx_offset_status[(offsetof(ArxIdentityV1,status)==12)?1:-1];
+typedef char arx_offset_os_errno[(offsetof(ArxIdentityV1,os_errno)==16)?1:-1];
+typedef char arx_offset_vfs_kind[(offsetof(ArxIdentityV1,vfs_kind)==20)?1:-1];
+typedef char arx_offset_dev_lo[(offsetof(ArxIdentityV1,dev_lo)==24)?1:-1];
+typedef char arx_offset_dev_hi[(offsetof(ArxIdentityV1,dev_hi)==28)?1:-1];
+typedef char arx_offset_ino_lo[(offsetof(ArxIdentityV1,ino_lo)==32)?1:-1];
+typedef char arx_offset_ino_hi[(offsetof(ArxIdentityV1,ino_hi)==36)?1:-1];
+typedef char arx_offset_nlink_lo[(offsetof(ArxIdentityV1,nlink_lo)==40)?1:-1];
+typedef char arx_offset_nlink_hi[(offsetof(ArxIdentityV1,nlink_hi)==44)?1:-1];
+typedef char arx_offset_mode[(offsetof(ArxIdentityV1,mode)==48)?1:-1];
+typedef char arx_offset_flags[(offsetof(ArxIdentityV1,flags)==52)?1:-1];
+typedef char arx_offset_reserved[(offsetof(ArxIdentityV1,reserved)==56)?1:-1];
+typedef char arx_size[(sizeof(ArxIdentityV1)==80)?1:-1];
+struct ArxAlignmentV1 { char c; ArxIdentityV1 v; };
+typedef char arx_align[(offsetof(struct ArxAlignmentV1,v)==4)?1:-1];
+SQLITE_API int sqlite3_arxcovo_bound_abi_v1(void);
+SQLITE_API int sqlite3_arxcovo_qualify_main_v1(sqlite3*);
+
 #define SQLITE_FCNTL_FILE_POINTER            7
 #define SQLITE_FCNTL_SYNC_OMITTED            8
 #define SQLITE_FCNTL_WIN32_AV_RETRY          9
```
