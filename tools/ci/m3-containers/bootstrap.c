#define _GNU_SOURCE
#include <arpa/inet.h>
#include <errno.h>
#include <fcntl.h>
#include <dirent.h>
#include <grp.h>
#include <limits.h>
#include <linux/capability.h>
#include <linux/magic.h>
#include <linux/openat2.h>
#include <poll.h>
#include <pwd.h>
#include <sched.h>
#include <shadow.h>
#include <signal.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/ioctl.h>
#include <sys/mount.h>
#include <sys/prctl.h>
#include <sys/resource.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/statfs.h>
#include <sys/statvfs.h>
#include <sys/syscall.h>
#include <sys/sysmacros.h>
#include <sys/types.h>
#include <sys/un.h>
#include <sys/wait.h>
#include <sys/xattr.h>
#include <termios.h>
#include <unistd.h>

#define CG "/sys/fs/cgroup"
#define SLICE CG "/system.slice"
#define UNIT SLICE "/arxd.service"
#define SUP UNIT "/supervisor"
#define RUNS UNIT "/runs"
#define LEAF RUNS "/fixture"
#define DAEMON 21071
#define BUILD 21072
#define BROKER_CAPS 0xc1u
#define SLOT "/data/workspace/ordinary"
#define PROBE "/fixture/probe"

static char *const tool_env[] = {
    "PATH=/opt/rust/bin:/usr/bin:/bin", "HOME=/data/home", "CARGO_HOME=/data/home",
    "LANG=C", "LC_ALL=C", "TZ=UTC", "LD_LIBRARY_PATH=/opt/rust/lib", NULL,
};
extern char **environ;

#define CHECK(test, category) do { if (!(test)) refuse(category); } while (0)

static void refuse(const char *category)
{
    dprintf(STDERR_FILENO, "R2 bootstrap %s errno=%d\n", category, errno);
    _exit(1);
}

static void join_path(char *out, size_t size, const char *base, const char *suffix)
{
    int written = snprintf(out, size, "%s%s", base, suffix);
    CHECK(written > 0 && (size_t)written < size, "path");
}

static void read_text(const char *path, char *text, size_t size)
{
    int fd = open(path, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    CHECK(fd >= 0, "read-open");
    ssize_t count = read(fd, text, size);
    CHECK(count >= 0 && (size_t)count < size && close(fd) == 0, "read-size");
    text[count] = '\0';
}

static void exact(const char *path, const char *expected, const char *category)
{
    char text[1024];
    read_text(path, text, sizeof(text));
    CHECK(strcmp(text, expected) == 0, category);
}

static void write_text(const char *path, const char *value)
{
    int fd = open(path, O_WRONLY | O_CLOEXEC | O_NOFOLLOW);
    size_t size = strlen(value);
    CHECK(fd >= 0 && write(fd, value, size) == (ssize_t)size && close(fd) == 0, "control-write");
}

static void pid_write(const char *path, pid_t pid)
{
    char text[32];
    CHECK(snprintf(text, sizeof(text), "%d", pid) > 0, "pid-render");
    write_text(path, text);
}

static void pid_exact(const char *path, pid_t pid)
{
    char text[32];
    CHECK(snprintf(text, sizeof(text), "%d\n", pid) > 0, "pid-render");
    exact(path, text, "sole-pid");
}

static void dir(const char *path, mode_t mode)
{
    CHECK(mkdir(path, mode) == 0 && chmod(path, mode) == 0, "mkdir");
}

static void identity(const char *path, uid_t uid, gid_t gid, mode_t mode, bool directory)
{
    struct stat facts;
    CHECK(lstat(path, &facts) == 0 && facts.st_uid == uid && facts.st_gid == gid
          && (facts.st_mode & 07777) == mode
          && (directory ? S_ISDIR(facts.st_mode) : S_ISREG(facts.st_mode)), "path-identity");
    CHECK(llistxattr(path, NULL, 0) == 0, "unexpected-acl-xattr");
}

static void expect_fs(const char *path, long magic, bool readonly)
{
    struct statfs filesystem;
    struct statvfs view;
    CHECK(statfs(path, &filesystem) == 0 && filesystem.f_type == magic
          && statvfs(path, &view) == 0 && !!(view.f_flag & ST_RDONLY) == readonly, "filesystem-view");
}

static void set_mount(const char *source, const char *target, const char *type,
                      unsigned long flags, const char *options)
{
    CHECK(mount(source, target, type, flags, options) == 0, "mount");
}

static void devnode(const char *root, const char *suffix, unsigned int major_id, unsigned int minor_id)
{
    char path[160];
    join_path(path, sizeof(path), root, suffix);
    CHECK(mknod(path, S_IFCHR | 0666, makedev(major_id, minor_id)) == 0 && chmod(path, 0666) == 0,
          "device-node");
}

static void reset_fds(void)
{
    CHECK(syscall(SYS_close_range, 3u, ~0u, 0u) == 0, "close-range");
    int null = open("/dev/null", O_RDWR | O_NOFOLLOW | O_CLOEXEC);
    CHECK(null == 3 && dup2(null, STDIN_FILENO) == STDIN_FILENO && close(null) == 0, "stdin");
}

static void check_fds(void)
{
    for (int fd = 3; fd < 128; ++fd)
        CHECK(fcntl(fd, F_GETFD) == -1 && errno == EBADF, "extra-fd");
    struct stat facts;
    CHECK(fstat(STDIN_FILENO, &facts) == 0 && S_ISCHR(facts.st_mode)
          && facts.st_rdev == makedev(1, 3), "stdin-device");
    for (int fd = STDOUT_FILENO; fd <= STDERR_FILENO; ++fd)
        CHECK(fstat(fd, &facts) == 0 && (S_ISFIFO(facts.st_mode) || S_ISSOCK(facts.st_mode)), "output-fd");
}

static void capabilities(unsigned int mask, bool ambient)
{
    CHECK(prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_CLEAR_ALL, 0, 0, 0) == 0, "ambient-clear");
    for (unsigned int cap = 0; cap <= 63; ++cap) {
        int current = prctl(PR_CAPBSET_READ, cap, 0, 0, 0);
        if (current < 0 && errno == EINVAL)
            break;
        CHECK(current >= 0, "cap-bounding-read");
        if (!((UINT64_C(1) << cap) & mask))
            CHECK(prctl(PR_CAPBSET_DROP, cap, 0, 0, 0) == 0, "cap-bounding-drop");
        else
            CHECK(current == 1, "cap-bounding-retained");
    }
    struct __user_cap_header_struct header = {_LINUX_CAPABILITY_VERSION_3, 0};
    struct __user_cap_data_struct data[2] = {{mask, mask, ambient ? mask : 0}, {0, 0, 0}};
    CHECK(syscall(SYS_capset, &header, data) == 0, "caps");
    for (unsigned int cap = 0; ambient && cap < 8; ++cap)
        if ((1u << cap) & mask)
            CHECK(prctl(PR_CAP_AMBIENT, PR_CAP_AMBIENT_RAISE, cap, 0, 0) == 0, "ambient");
    CHECK(prctl(PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == 0, "nnp");
}

static void status_value(const char *status, const char *key, unsigned long long want, int base)
{
    const char *line = strstr(status, key);
    CHECK(line && (line == status || line[-1] == '\n'), "status-key");
    char *end;
    unsigned long long value = strtoull(line + strlen(key), &end, base);
    CHECK(value == want && *end == '\n', "status-value");
}

static void security(unsigned int mask, bool ambient)
{
    char status[8192];
    struct rlimit limit;
    read_text("/proc/self/status", status, sizeof(status));
    status_value(status, "CapInh:", ambient ? mask : 0, 16);
    status_value(status, "CapPrm:", mask, 16);
    status_value(status, "CapEff:", mask, 16);
    status_value(status, "CapBnd:", mask, 16);
    status_value(status, "CapAmb:", ambient ? mask : 0, 16);
    status_value(status, "NoNewPrivs:", 1, 10);
    status_value(status, "Umask:", 0077, 8);
    CHECK(getrlimit(RLIMIT_CORE, &limit) == 0 && limit.rlim_cur == 0 && limit.rlim_max == 0, "core-limit");
    CHECK(getrlimit(RLIMIT_NOFILE, &limit) == 0 && limit.rlim_cur == 128 && limit.rlim_max == 128, "fd-limit");
    check_fds();
}

static void child_ok(pid_t pid)
{
    int status;
    CHECK(waitpid(pid, &status, 0) == pid && WIFEXITED(status) && WEXITSTATUS(status) == 0, "child");
}

static pid_t fork_child(void)
{
    pid_t child = fork();
    CHECK(child >= 0, "fork");
    return child;
}

static void enter_role(uid_t uid)
{
    CHECK(setgroups(0, NULL) == 0 && setresgid(uid, uid, uid) == 0 && setresuid(uid, uid, uid) == 0
          && getuid() == uid && geteuid() == uid && getgid() == uid && getegid() == uid
          && getgroups(0, NULL) == 0, "role");
    struct __user_cap_header_struct header = {_LINUX_CAPABILITY_VERSION_3, 0};
    struct __user_cap_data_struct data[2] = {{0}};
    CHECK(syscall(SYS_capget, &header, data) == 0, "role-capget");
    data[0].effective = 1u << CAP_SETPCAP;
    CHECK(syscall(SYS_capset, &header, data) == 0, "role-drop-authority");
}

static void deny_write(const char *path)
{
    int fd = open(path, O_WRONLY | O_CLOEXEC | O_NOFOLLOW);
    CHECK(fd == -1 && (errno == EACCES || errno == EPERM || errno == EROFS), "expected-denial");
}

static void check_domain(const char *base, bool empty, bool enabled)
{
    char path[160];
    char value[256];
    join_path(path, sizeof(path), base, "/cgroup.type");
    exact(path, "domain\n", "cgroup-domain");
    if (empty) {
        join_path(path, sizeof(path), base, "/cgroup.procs");
        exact(path, "", "cgroup-parent-pids");
    }
    join_path(path, sizeof(path), base, enabled ? "/cgroup.subtree_control" : "/cgroup.controllers");
    read_text(path, value, sizeof(value));
    bool memory = false, pids = false;
    char *save;
    for (char *word = strtok_r(value, " \n", &save); word; word = strtok_r(NULL, " \n", &save)) {
        memory |= strcmp(word, "memory") == 0;
        pids |= strcmp(word, "pids") == 0;
    }
    CHECK(memory && pids, "cgroup-controllers");
}

static void host_cgroup(const char *id, const char *instance, char *relative, size_t length)
{
    char rendered[160], outer[160], path[256];
    CHECK(snprintf(relative, length, "/system.slice/arx-r2-%s-%s.service", id, instance) > 0,
          "cgroup-name");
    join_path(outer, sizeof(outer), CG, relative);
    CHECK(snprintf(rendered, sizeof(rendered), "0::%s\n", relative) > 0, "cgroup-name");
    exact("/proc/self/cgroup", rendered, "systemd-placement");
    struct stat facts;
    CHECK(lstat(outer, &facts) == 0 && S_ISDIR(facts.st_mode) && facts.st_uid == 0
          && facts.st_gid == 0 && (facts.st_mode & 07777) == 0755, "systemd-cgroup-owner");
    CHECK(unshare(CLONE_NEWCGROUP) == 0, "cgroup-namespace");
    join_path(path, sizeof(path), outer, "/init");
    dir(path, 0755);
    join_path(path, sizeof(path), outer, "/init/cgroup.procs");
    pid_write(path, getpid());
    if (instance[0] != 'b')
        return;
    join_path(path, sizeof(path), outer, "/cgroup.subtree_control");
    write_text(path, "+memory +pids");
    check_domain(outer, true, true);
    join_path(path, sizeof(path), outer, "/system.slice");
    dir(path, 0755);
    check_domain(path, true, false);
    join_path(path, sizeof(path), outer, "/system.slice/cgroup.subtree_control");
    write_text(path, "+memory +pids");
    join_path(path, sizeof(path), outer, "/system.slice/arxd.service");
    dir(path, 0755);
    char marker[2];
    CHECK(setxattr(path, "user.delegate", "1", 1, XATTR_CREATE) == 0
          && getxattr(path, "user.delegate", marker, sizeof(marker)) == 1 && marker[0] == '1', "delegate-marker");
    join_path(path, sizeof(path), outer, "/system.slice/arxd.service/supervisor");
    dir(path, 0755);
}

static void mount_root(const char *id, char node)
{
    const unsigned long fence = MS_NOSUID | MS_NODEV | MS_NOEXEC;
    char base[96], root[128], source[128], target[160];
    CHECK(snprintf(base, sizeof(base), "/var/lib/arx-r2-%s", id) > 0, "mount-base");
    join_path(root, sizeof(root), base, "/root");
    CHECK(snprintf(source, sizeof(source), "%s/%c", base, node) > 0, "data-path");
    set_mount(NULL, "/", NULL, MS_REC | MS_PRIVATE, NULL);
    set_mount(root, root, NULL, MS_BIND, NULL);
    expect_fs(root, EXT4_SUPER_MAGIC, true);
    join_path(target, sizeof(target), root, "/data");
    set_mount(source, target, NULL, MS_BIND, NULL);
    expect_fs(target, EXT4_SUPER_MAGIC, false);
    join_path(target, sizeof(target), root, "/proc");
    set_mount("proc", target, "proc", fence, "hidepid=2");
    join_path(target, sizeof(target), root, "/run");
    set_mount("tmpfs", target, "tmpfs", fence, "size=4194304,nr_inodes=256,mode=0755");
    join_path(target, sizeof(target), root, "/tmp");
    set_mount("tmpfs", target, "tmpfs", fence, "size=8388608,nr_inodes=512,mode=1777");
    join_path(target, sizeof(target), root, "/dev");
    set_mount("tmpfs", target, "tmpfs", MS_NOSUID | MS_NOEXEC, "size=1048576,nr_inodes=64,mode=0755");
    char device[160];
    join_path(device, sizeof(device), root, "/dev/pts");
    dir(device, 0755);
    set_mount("devpts", device, "devpts", MS_NOSUID | MS_NOEXEC, "newinstance,ptmxmode=0666,mode=0620,max=8");
    join_path(device, sizeof(device), root, "/dev/ptmx");
    CHECK(symlink("pts/ptmx", device) == 0, "ptmx");
    devnode(root, "/dev/null", 1, 3);
    devnode(root, "/dev/zero", 1, 5);
    devnode(root, "/dev/random", 1, 8);
    devnode(root, "/dev/tty", 5, 0);
    join_path(target, sizeof(target), root, CG);
    set_mount("cgroup2", target, "cgroup2", fence, NULL);
    if (node == 'b') {
        join_path(target, sizeof(target), root, UNIT);
        set_mount(target, target, NULL, MS_BIND, NULL);
    }
    join_path(target, sizeof(target), root, CG);
    set_mount(NULL, target, NULL, MS_REMOUNT | MS_BIND | MS_RDONLY | fence, NULL);
    CHECK(chdir(root) == 0 && syscall(SYS_pivot_root, ".", "put_old") == 0
          && chdir("/") == 0 && umount2("/put_old", MNT_DETACH) == 0, "pivot");
}

static void mount_inventory(bool broker)
{
    char inventory[8192];
    read_text("/proc/self/mountinfo", inventory, sizeof(inventory));
    const char *const targets[] = {"/", "/data", "/proc", "/run", "/tmp", "/dev", "/dev/pts", CG, UNIT};
    const char *const roots[] = {"/", "/", "/", "/", "/", "/", "/", "/", "/system.slice/arxd.service"};
    const char *const types[] = {"ext4", "ext4", "proc", "tmpfs", "tmpfs", "tmpfs", "devpts", "cgroup2", "cgroup2"};
    unsigned int hits = 0;
    int mount_id[9] = {0};
    char *save;
    for (char *line = strtok_r(inventory, "\n", &save); line; line = strtok_r(NULL, "\n", &save)) {
        int id, parent, end = 0;
        char root[96], target[96], options[96], type[24];
        CHECK(sscanf(line, "%d %d %*s %95s %95s %95s%n", &id, &parent, root, target, options, &end) == 5
              && strncmp(line + end, " - ", 3) == 0
              && sscanf(line + end + 3, "%23s", type) == 1, "private-mountinfo");
        size_t i = 0;
        while (i < 8u + broker && strcmp(target, targets[i]))
            ++i;
        CHECK(i < 8u + broker && !(hits & (1u << i)) && id > 0
              && strcmp(root, roots[i]) == 0 && strcmp(type, types[i]) == 0
              && (i == 0 || i == 7 ? strncmp(options, "ro,", 3) == 0 : strncmp(options, "rw,", 3) == 0),
              "mount-instance");
        hits |= 1u << i;
        mount_id[i] = id;
    }
    CHECK(hits == ((1u << (8u + broker)) - 1u) && (!broker || mount_id[7] != mount_id[8]), "mount-count");
    expect_fs(CG, CGROUP2_SUPER_MAGIC, true);
    if (broker)
        expect_fs(UNIT, CGROUP2_SUPER_MAGIC, false);
    expect_fs("/proc", PROC_SUPER_MAGIC, false);
    expect_fs("/dev/pts", DEVPTS_SUPER_MAGIC, false);
}


static int channel_pair(int pair[2])
{
    return socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0, pair);
}

static void peer(int socket_fd, pid_t expected)
{
    struct ucred credential;
    socklen_t length = sizeof(credential);
    CHECK(getsockopt(socket_fd, SOL_SOCKET, SO_PEERCRED, &credential, &length) == 0
          && length == sizeof(credential) && credential.pid == expected && credential.uid == 0
          && credential.gid == 0, "peer-pid");
}

static int bind_socket(const char *path)
{
    struct sockaddr_un address = {.sun_family = AF_UNIX};
    join_path(address.sun_path, sizeof(address.sun_path), path, "");
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    CHECK(fd >= 0 && bind(fd, (struct sockaddr *)&address, sizeof(address)) == 0
          && listen(fd, 2) == 0, "exclusive-bind");
    return fd;
}

static void socket_probes(void)
{
    const char *path = "/run/r2/inert.sock";
    int listener = bind_socket(path);
    struct sockaddr_un address = {.sun_family = AF_UNIX, .sun_path = "/run/r2/inert.sock"};
    int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    CHECK(fd >= 0 && bind(fd, (struct sockaddr *)&address, sizeof(address)) < 0 && errno == EADDRINUSE,
          "exclusive-rebind");
    CHECK(connect(fd, (struct sockaddr *)&address, sizeof(address)) == 0, "in-namespace-connect");
    int accepted = accept4(listener, NULL, NULL, SOCK_CLOEXEC);
    CHECK(accepted >= 0, "in-namespace-accept");
    peer(fd, 1);
    peer(accepted, 1);
    CHECK(close(accepted) == 0 && close(fd) == 0 && close(listener) == 0, "peer-close");
    /* Leave the inode in place. The second boot must get a fresh /run. */
}

static void pty_probe(void)
{
    static const char sentinel[] = "r2-pty\n";
    int master = posix_openpt(O_RDWR | O_NOCTTY | O_CLOEXEC);
    CHECK(master >= 0 && grantpt(master) == 0 && unlockpt(master) == 0, "pty-master");
    char name[64];
    CHECK(ptsname_r(master, name, sizeof(name)) == 0 && strcmp(name, "/dev/pts/0") == 0, "private-pts");
    int barrier[2];
    CHECK(pipe2(barrier, O_CLOEXEC) == 0, "pty-barrier");
    pid_t child = fork_child();
    if (child == 0) {
        CHECK(close(barrier[0]) == 0 && close(master) == 0 && setsid() == getpid(), "pty-session");
        int slave = open(name, O_RDWR | O_NOCTTY | O_CLOEXEC | O_NOFOLLOW);
        CHECK(slave >= 0 && ioctl(slave, TIOCSCTTY, 0) == 0, "pty-controlling");
        int terminal = open("/dev/tty", O_RDWR | O_CLOEXEC | O_NOFOLLOW);
        struct termios mode;
        CHECK(terminal >= 0 && tcgetsid(terminal) == getsid(0) && tcgetpgrp(terminal) == getpgrp()
              && tcgetattr(terminal, &mode) == 0, "pty-mode");
        mode.c_lflag &= ~(tcflag_t)(ECHO | ECHONL);
        CHECK(tcsetattr(terminal, TCSANOW, &mode) == 0 && write(barrier[1], "1", 1) == 1, "pty-ready");
        char actual[sizeof(sentinel)];
        CHECK(read(terminal, actual, sizeof(actual)) == (ssize_t)sizeof(sentinel) - 1
              && memcmp(actual, sentinel, sizeof(sentinel) - 1) == 0, "pty-transfer");
        _exit(0);
    }
    char byte;
    CHECK(close(barrier[1]) == 0 && read(barrier[0], &byte, 1) == 1 && byte == '1'
          && close(barrier[0]) == 0
          && write(master, sentinel, sizeof(sentinel) - 1) == (ssize_t)sizeof(sentinel) - 1, "pty-paste");
    child_ok(child);
    struct pollfd reading = {master, POLLIN, 0};
    CHECK(poll(&reading, 1, 0) >= 0 && !(reading.revents & POLLIN) && close(master) == 0, "pty-echo");
}

static void workspace(void)
{
    identity("/data/workspace", 0, 0, 0755, true);
    identity(SLOT, 0, 0, 0755, true);
    identity(SLOT "/src", 0, 0, 0755, true);
    identity(SLOT "/out", BUILD, BUILD, 0700, true);
    identity("/data/home", BUILD, BUILD, 0700, true);
    int base = open("/data/workspace", O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC);
    struct open_how how = {.flags = O_RDONLY | O_DIRECTORY | O_CLOEXEC,
                           .resolve = RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS | RESOLVE_NO_XDEV};
    CHECK(base >= 0, "workspace-root");
    int slot = (int)syscall(SYS_openat2, base, "ordinary", &how, sizeof(how));
    CHECK(slot >= 0 && close(slot) == 0 && close(base) == 0, "workspace-slot-mount");
}

static void role_inventory(void)
{
    static const struct { const char *name; uid_t uid; } roles[] = {
        {"arx-owner", 21070}, {"arx-daemon", DAEMON}, {"arx-build", BUILD}, {"arx-agent", 21073},
    };
    identity("/etc/passwd", 0, 0, 0644, false);
    identity("/etc/group", 0, 0, 0644, false);
    identity("/etc/shadow", 0, 0, 0400, false);
    exact("/etc/nsswitch.conf", "passwd: files\ngroup: files\nshadow: files\nhosts: files\n", "nss-files-only");
    for (size_t i = 0; i < sizeof(roles)/sizeof(roles[0]); ++i) {
        const char *name = roles[i].name;
        uid_t uid = roles[i].uid;
        struct passwd *user = getpwnam(name);
        CHECK(user && user->pw_uid == uid && user->pw_gid == uid && strcmp(user->pw_shell, "/bin/sh") == 0,
              "role-passwd");
        user = getpwuid(uid);
        CHECK(user && strcmp(user->pw_name, name) == 0, "role-passwd-reverse");
        struct group *group = getgrgid(uid);
        CHECK(group && strcmp(group->gr_name, name) == 0 && group->gr_mem[0] == NULL, "role-group");
        struct spwd *shadow = getspnam(name);
        CHECK(shadow && strcmp(shadow->sp_pwdp, "!") == 0 && shadow->sp_expire == 1, "role-locked-expired");
    }
}

static void build_checks(void)
{
    security(0, false);
    exact("/proc/self/cgroup", "0::/system.slice/arxd.service/runs/fixture\n", "build-leaf");
    deny_write(CG "/cgroup.procs");
    deny_write(UNIT "/cgroup.subtree_control");
    deny_write(LEAF "/cgroup.procs");
    deny_write(LEAF "/cgroup.kill");
    deny_write(LEAF "/memory.max");
    deny_write(LEAF "/memory.swap.max");
    deny_write(LEAF "/pids.max");
    CHECK(access("/data/private", R_OK | X_OK) == -1 && errno == EACCES, "private-denial");
    CHECK(access("/run/r2/inert.sock", W_OK) == -1 && errno == EACCES, "socket-denial");
    CHECK(rename(SLOT, "/data/workspace/replaced") == -1 && errno == EACCES, "slot-denial");
    int fd = open(SLOT "/out/r2", O_CREAT | O_EXCL | O_WRONLY | O_NOFOLLOW, 0600);
    CHECK(fd >= 0 && close(fd) == 0 && unlink(SLOT "/out/r2") == 0, "out-write");
    const char *const tools[] = {"/opt/rust/bin/cargo", "/opt/rust/bin/rustc", "/usr/bin/cc"};
    for (size_t i = 0; i < sizeof(tools)/sizeof(tools[0]); ++i) {
        pid_t child = fork_child();
        if (child == 0) {
            char *const args[] = {(char *)tools[i], "--version", NULL};
            int null = open("/dev/null", O_WRONLY | O_CLOEXEC | O_NOFOLLOW);
            CHECK(null >= 3 && dup2(null, STDOUT_FILENO) == STDOUT_FILENO && close(null) == 0, "version-output");
            execve(tools[i], args, tool_env);
            refuse("version-exec");
        }
        child_ok(child);
    }
}

static void report(const char *path, const char *value);
static void wait_path(const char *path);

static void broker(void)
{
    CHECK(getuid() == 0 && geteuid() == 0 && getgroups(0, NULL) == 0, "broker-uid");
    security(BROKER_CAPS, true);
    mount_inventory(true);
    exact("/proc/self/cgroup", "0::/system.slice/arxd.service/supervisor\n", "broker-membership");
    pid_exact(SUP "/cgroup.procs", getpid());
    check_domain(UNIT, true, false);
    char marker[2];
    CHECK(getxattr(UNIT, "user.delegate", marker, sizeof(marker)) == 1 && marker[0] == '1'
          && access(RUNS, F_OK) < 0 && errno == ENOENT, "delegate-marker-fresh-runs");
    deny_write(CG "/cgroup.procs");
    write_text(UNIT "/cgroup.subtree_control", "+memory +pids");
    check_domain(UNIT, true, true);
    dir(RUNS, 0700);
    write_text(RUNS "/cgroup.subtree_control", "+memory +pids");
    check_domain(RUNS, true, true);
    dir(LEAF, 0700);
    write_text(LEAF "/memory.max", "134217728");
    write_text(LEAF "/memory.swap.max", "0");
    write_text(LEAF "/pids.max", "32");
    exact(LEAF "/memory.max", "134217728\n", "leaf-memory");
    exact(LEAF "/memory.swap.max", "0\n", "leaf-swap");
    exact(LEAF "/pids.max", "32\n", "leaf-pids");
    report("/run/r2/kill-leaf", "r2-kill-v1\n");
    wait_path("/run/r2/kill-target");
    exact(LEAF "/cgroup.events", "populated 1\nfrozen 0\n", "kill-target-populated");
    write_text(LEAF "/cgroup.kill", "1");
    wait_path("/run/r2/kill-reaped");
    exact(LEAF "/cgroup.events", "populated 0\nfrozen 0\n", "kill-target-empty");
    report("/run/r2/broker", "r2-broker-v1\n");
    while (true)
        pause();
}

static void device_inventory(void)
{
    const char *const names[] = {"null", "zero", "random", "tty", "ptmx", "pts"};
    unsigned int seen = 0;
    DIR *directory = opendir("/dev");
    CHECK(directory, "device-inventory");
    for (struct dirent *entry; (entry = readdir(directory));) {
        if (!strcmp(entry->d_name, ".") || !strcmp(entry->d_name, ".."))
            continue;
        size_t i = 0;
        while (i < 6 && strcmp(entry->d_name, names[i]))
            ++i;
        CHECK(i < 6 && !(seen & (1u << i)), "extra-device");
        seen |= 1u << i;
    }
    CHECK(closedir(directory) == 0 && seen == 63, "missing-device");
}

static void wait_path(const char *path)
{
    for (unsigned int i = 0; i < 500; ++i) {
        if (access(path, F_OK) == 0)
            return;
        CHECK(errno == ENOENT, "await-path");
        usleep(10000);
    }
    refuse("await-path-timeout");
}

static void report(const char *path, const char *value)
{
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0644);
    size_t size = strlen(value);
    CHECK(fd >= 0 && fchmod(fd, 0644) == 0 && write(fd, value, size) == (ssize_t)size
          && close(fd) == 0, "report");
}

static volatile sig_atomic_t stopping;

static void stop(int signal_number)
{
    (void)signal_number;
    stopping = 1;
}

static void supervise(void)
{
    struct sigaction action = {.sa_handler = stop};
    CHECK(sigemptyset(&action.sa_mask) == 0 && sigaction(SIGTERM, &action, NULL) == 0
          && sigaction(SIGINT, &action, NULL) == 0, "signals");
    while (!stopping) {
        int status;
        CHECK(waitpid(-1, &status, 0) == -1 && errno == EINTR && stopping, "unexpected-probe-exit");
    }
    _exit(0);
}

static void network(char node, uid_t uid, char generation);

static void init(const char *id, char node, int outside_peer, char generation)
{
    CHECK(getpid() == 1 && prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) == 0, "pid-one");
    CHECK(setgroups(0, NULL) == 0, "init-groups");
    peer(outside_peer, 0);
    CHECK(close(outside_peer) == 0, "parent-peer-close");
    mount_root(id, node);
    mount_inventory(node == 'b');
    reset_fds();
    device_inventory();
    identity("/", 0, 0, 0755, true);
    role_inventory();
    exact("/data/probe/persistent", "r2-persistent-v1\n", "persistent-image");
    dir("/run/r2", 0700);
    socket_probes();
    pty_probe();
    if (node == 'b') {
        workspace();
        int barrier[2];
        CHECK(channel_pair(barrier) == 0, "broker-barrier");
        pid_t child = fork_child();
        if (child == 0) {
            char ready;
            peer(barrier[1], 1);
            CHECK(close(barrier[0]) == 0 && read(barrier[1], &ready, 1) == 1 && ready == '1'
                  && close(barrier[1]) == 0 && setgroups(0, NULL) == 0, "broker-release");
            capabilities(BROKER_CAPS, true);
            char *const args[] = {PROBE, "broker", NULL};
            execve(PROBE, args, tool_env);
            refuse("broker-exec");
        }
        CHECK(close(barrier[1]) == 0, "barrier-direction");
        pid_write(SUP "/cgroup.procs", child);
        pid_exact(SUP "/cgroup.procs", child);
        CHECK(write(barrier[0], "1", 1) == 1 && close(barrier[0]) == 0, "broker-barrier-send");
        wait_path("/run/r2/kill-leaf");
        pid_t disposable = fork_child();
        if (disposable == 0) {
            pid_write(LEAF "/cgroup.procs", getpid());
            capabilities(0, false);
            report("/run/r2/kill-target", "r2-kill-v1\n");
            while (true)
                pause();
        }
        int status;
        CHECK(waitpid(disposable, &status, 0) == disposable && WIFSIGNALED(status)
              && WTERMSIG(status) == SIGKILL, "kill-target-status");
        report("/run/r2/kill-reaped", "r2-kill-v1\n");
        wait_path("/run/r2/broker");
        const char *ready = generation == '0' ? "/data/probe/build/ready0" : "/data/probe/build/ready1";
        pid_t build = fork_child();
        if (build == 0) {
            pid_write(LEAF "/cgroup.procs", getpid());
            CHECK(prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0) == 0, "build-keepcaps");
            enter_role(BUILD);
            capabilities(0, false);
            build_checks();
            report(ready, "r2-build-v1\n");
            network(node, BUILD, generation);
        }
        wait_path(ready);
    }
    if (node == 'c') {
        /* Port 53 is only a synthetic listener in C's private network namespace. */
        write_text("/proc/sys/net/ipv4/ip_unprivileged_port_start", "53");
        exact("/proc/sys/net/ipv4/ip_unprivileged_port_start", "53\n", "dns-port-scope");
    }
    pid_t daemon = fork_child();
    if (daemon == 0) {
        CHECK(prctl(PR_SET_KEEPCAPS, 1, 0, 0, 0) == 0, "daemon-keepcaps");
        enter_role(DAEMON);
        capabilities(0, false);
        security(0, false);
        network(node, DAEMON, generation);
    }
    capabilities(0, false);
    security(0, false);
    report(generation == '0' ? "/data/probe/boot0" : "/data/probe/boot1", "r2-init-v1\n");
    supervise();
}

int main(int argc, char **argv)
{
    CHECK(getuid() == 0 && geteuid() == 0 && getgid() == 0 && getegid() == 0, "root-identity");
    umask(0077);
    if (argc == 2 && strcmp(argv[1], "broker") == 0) {
        broker();
        return 1;
    }
    CHECK(argc == 4 && strcmp(argv[1], "boot") == 0 && strlen(argv[2]) == 16
          && strspn(argv[2], "0123456789abcdef") == 16
          && (!strcmp(argv[3], "a0") || !strcmp(argv[3], "b0") || !strcmp(argv[3], "c0")
              || !strcmp(argv[3], "b1")), "arguments");
    char net[128], group[160];
    CHECK(snprintf(net, sizeof(net), "/run/netns/r2-%s-%c", argv[2], argv[3][0]) > 0, "netns-path");
    int ns = open(net, O_RDONLY | O_CLOEXEC | O_NOFOLLOW);
    CHECK(ns >= 0 && setns(ns, CLONE_NEWNET) == 0 && close(ns) == 0, "enter-netns");
    host_cgroup(argv[2], argv[3], group, sizeof(group));
    CHECK(unshare(CLONE_NEWNS | CLONE_NEWUTS | CLONE_NEWIPC | CLONE_NEWPID) == 0
          && sethostname("arx-r2", 6) == 0, "private-namespaces");
    int pair[2];
    CHECK(channel_pair(pair) == 0, "outside-peer");
    pid_t child = fork_child();
    if (child == 0) {
        CHECK(close(pair[0]) == 0, "outside-peer-close");
        init(argv[2], argv[3][0], pair[1], argv[3][1]);
    }
    CHECK(close(pair[0]) == 0 && close(pair[1]) == 0, "outside-close");
    reset_fds();
    capabilities(0, false);
    child_ok(child);
    return 0;
}


static int endpoint(const char *address, unsigned int port, int transport, struct sockaddr_storage *out)
{
    memset(out, 0, sizeof(*out));
    if (strchr(address, ':')) {
        struct sockaddr_in6 *ipv6 = (struct sockaddr_in6 *)out;
        ipv6->sin6_family = AF_INET6;
        ipv6->sin6_port = htons((uint16_t)port);
        CHECK(inet_pton(AF_INET6, address, &ipv6->sin6_addr) == 1, "ipv6-address");
    } else {
        struct sockaddr_in *ipv4 = (struct sockaddr_in *)out;
        ipv4->sin_family = AF_INET;
        ipv4->sin_port = htons((uint16_t)port);
        CHECK(inet_pton(AF_INET, address, &ipv4->sin_addr) == 1, "ipv4-address");
    }
    int fd = socket(out->ss_family, transport | SOCK_CLOEXEC | SOCK_NONBLOCK, 0);
    CHECK(fd >= 0, "network-socket");
    if (out->ss_family == AF_INET6) {
        int enabled = 1;
        CHECK(setsockopt(fd, IPPROTO_IPV6, IPV6_V6ONLY, &enabled, sizeof(enabled)) == 0, "ipv6-only");
    }
    return fd;
}

static socklen_t address_size(const struct sockaddr_storage *address)
{
    return address->ss_family == AF_INET ? sizeof(struct sockaddr_in) : sizeof(struct sockaddr_in6);
}

struct network_case {
    const char *address;
    unsigned int port;
    int transport;
    bool allowed;
};

static int listen_case(const struct network_case *probe)
{
    struct sockaddr_storage address;
    int fd = endpoint(probe->address, probe->port, probe->transport, &address);
    CHECK(bind(fd, (struct sockaddr *)&address, address_size(&address)) == 0, "listener-bind");
    CHECK(probe->transport == SOCK_DGRAM || listen(fd, 4) == 0, "listener-start");
    return fd;
}

static bool poll_event(int fd, short event, int timeout)
{
    struct pollfd watch = {fd, event, 0};
    int count = poll(&watch, 1, timeout);
    CHECK(count >= 0, "network-poll");
    return count == 1;
}

static void attempt(const struct network_case *probe)
{
    struct sockaddr_storage address;
    int fd = endpoint(probe->address, probe->port, probe->transport, &address);
    int result = connect(fd, (struct sockaddr *)&address, address_size(&address));
    if (probe->transport == SOCK_STREAM) {
        CHECK(result == 0 || errno == EINPROGRESS, "network-route");
        if (!probe->allowed) {
            CHECK(result < 0 && !poll_event(fd, POLLOUT, 100), "network-deny-tcp");
            CHECK(close(fd) == 0, "network-close");
            return;
        }
        CHECK(poll_event(fd, POLLOUT, 500), "network-connected");
        int error;
        socklen_t n = sizeof(error);
        CHECK(getsockopt(fd, SOL_SOCKET, SO_ERROR, &error, &n) == 0 && error == 0, "network-connect-result");
    } else {
        CHECK(result == 0, "network-route");
    }
    uint64_t nonce, echoed;
    CHECK(syscall(SYS_getrandom, &nonce, sizeof(nonce), 0) == (ssize_t)sizeof(nonce)
          && send(fd, &nonce, sizeof(nonce), MSG_NOSIGNAL) == (ssize_t)sizeof(nonce), "network-nonce");
    if (probe->allowed) {
        CHECK(poll_event(fd, POLLIN, 500) && recv(fd, &echoed, sizeof(echoed), MSG_WAITALL) == (ssize_t)sizeof(echoed)
              && echoed == nonce, "network-echo");
    } else {
        CHECK(!poll_event(fd, POLLIN, 100), "network-deny-udp");
    }
    CHECK(close(fd) == 0, "network-close");
}

static void serve(const struct network_case *probes, size_t count, bool synthetic, const char *receipt)
{
    struct pollfd fds[8];
    CHECK(count <= sizeof(fds)/sizeof(fds[0]), "listener-count");
    for (size_t i = 0; i < count; ++i)
        fds[i] = (struct pollfd){listen_case(&probes[i]), POLLIN, 0};
    pid_t control = fork_child();
    if (control == 0) {
        for (size_t i = 0; i < count; ++i)
            CHECK(close(fds[i].fd) == 0, "control-fd");
        for (size_t i = 0; i < count; ++i) {
            struct network_case allowed = probes[i];
            allowed.allowed = true;
            attempt(&allowed);
        }
        _exit(0);
    }
    bool proven = false;
    while (true) {
        if (!proven) {
            int status;
            pid_t completed = waitpid(control, &status, WNOHANG);
            CHECK(completed >= 0, "control-wait");
            if (completed) {
                CHECK(WIFEXITED(status) && WEXITSTATUS(status) == 0, "listener-positive-control");
                report(receipt, "r2-listeners-v1\n");
                proven = true;
            }
        }
        if (synthetic && access("/data/probe/daemon/finish", F_OK) == 0) {
            exact("/data/probe/daemon/finish", "r2-network-v1\n", "listener-finish");
            for (size_t i = 0; i < count; ++i) {
                if (!probes[i].allowed) {
                    CHECK(close(fds[i].fd) == 0, "listener-retire");
                    fds[i].fd = -1;
                }
            }
            report("/data/probe/daemon/closed", "r2-network-v1\n");
            synthetic = false;
        }
        CHECK(poll(fds, count, 20) >= 0, "listen-poll");
        for (size_t i = 0; i < count; ++i) {
            if (!(fds[i].revents & POLLIN))
                continue;
            uint64_t nonce;
            struct sockaddr_storage from;
            socklen_t length = sizeof(from);
            int fd = fds[i].fd;
            if (probes[i].transport == SOCK_DGRAM) {
                CHECK(recvfrom(fd, &nonce, sizeof(nonce), 0, (struct sockaddr *)&from, &length) == (ssize_t)sizeof(nonce)
                      && sendto(fd, &nonce, sizeof(nonce), 0, (struct sockaddr *)&from, length) == (ssize_t)sizeof(nonce),
                      "udp-control");
            } else {
                fd = accept4(fd, NULL, NULL, SOCK_CLOEXEC | SOCK_NONBLOCK);
                CHECK(fd >= 0 && poll_event(fd, POLLIN, 500)
                      && recv(fd, &nonce, sizeof(nonce), MSG_WAITALL) == (ssize_t)sizeof(nonce)
                      && send(fd, &nonce, sizeof(nonce), MSG_NOSIGNAL) == (ssize_t)sizeof(nonce)
                      && close(fd) == 0, "tcp-control");
            }
        }
    }
}

static void network(char node, uid_t uid, char generation)
{
    static const struct network_case a[] = {
        {"198.18.72.11", 22171, SOCK_STREAM, true},
    };
    static const struct network_case c[] = {
        {"198.18.72.13", 22172, SOCK_STREAM, true},
        {"198.18.72.13", 22174, SOCK_STREAM, false},
        {"198.18.72.13", 53, SOCK_STREAM, false},
        {"198.18.72.13", 53, SOCK_DGRAM, false},
        {"192.0.2.72", 22174, SOCK_STREAM, false},
        {"169.254.254.72", 22174, SOCK_STREAM, false},
        {"fd42:72::13", 22174, SOCK_STREAM, false},
    };
    static const struct network_case build_route = {"198.18.72.12", 22173, SOCK_STREAM, true};
    if (node == 'a') {
        pid_t actor = fork_child();
        if (actor == 0) {
            const char *trigger = "/data/probe/daemon/build-go";
            for (unsigned int i = 0; i < 2500 && access(trigger, F_OK) < 0; ++i) {
                CHECK(errno == ENOENT, "build-probe-trigger");
                usleep(10000);
            }
            exact(trigger, "r2-network-v1\n", "build-probe-trigger");
            attempt(&build_route);
            report("/data/probe/daemon/build-result", "r2-network-v1\n");
            while (true)
                pause();
        }
        serve(a, sizeof(a)/sizeof(a[0]), false, "/data/probe/daemon/listeners");
    }
    if (node == 'c')
        serve(c, sizeof(c)/sizeof(c[0]), true, "/data/probe/daemon/listeners");
    bool build = uid == BUILD;
    if (!build && fork_child() == 0)
        serve(&build_route, 1, false, generation == '0'
              ? "/data/probe/daemon/listeners0" : "/data/probe/daemon/listeners1");
    const char *trigger = build ? "/data/probe/build/go" : "/data/probe/daemon/go";
    const char *results = build ? "/data/probe/build/result" : "/data/probe/daemon/result";
    wait_path(trigger);
    exact(trigger, "r2-network-v1\n", "network-trigger");
    attempt(a);
    for (size_t i = 0; i < sizeof(c)/sizeof(c[0]); ++i)
        attempt(&c[i]);
    const struct network_case local = {"198.18.72.14", 22174, SOCK_STREAM, false};
    attempt(&local);
    report(results, "r2-network-v1\n");
    while (true)
        pause();
}
