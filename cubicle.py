#!/usr/bin/env python3

import argparse
import json
import os
import random
import re
import shlex
import shutil
import socket
import subprocess
import sys
import time
import urllib.request
from abc import ABC, abstractmethod
from pathlib import Path
from typing import (
    Any,
    IO,
    Iterable,
    Literal,
    Optional,
    Sequence,
    TypedDict,
    TypeVar,
    Union,
)

T = TypeVar("T")

PackageName = str

PackageSpec = TypedDict(
    "PackageSpec",
    {
        "build-depends": set[PackageName],
        "depends": set[PackageName],
        "dir": Path,
        "origin": str,
        "update": Optional[Path],
        "test": Optional[Path],
    },
)


HOSTNAME = socket.gethostname()
HOME = Path.home()
XDG_CACHE_HOME = Path(os.environ.get("XDG_CACHE_HOME", HOME / ".cache"))
XDG_DATA_HOME = Path(os.environ.get("XDG_DATA_HOME", HOME / ".local" / "share"))
SCRIPT_NAME = os.path.basename(sys.argv[0])
SCRIPT_PATH = Path(os.path.dirname(os.path.realpath(__file__)))
HOME_DIRS = XDG_CACHE_HOME / "cubicle" / "home"
WORK_DIRS = XDG_DATA_HOME / "cubicle" / "work"
PACKAGE_CACHE = XDG_CACHE_HOME / "cubicle" / "packages"
CODE_PACKAGE_DIR = SCRIPT_PATH / "packages"
USER_PACKAGE_DIR = XDG_DATA_HOME / "cubicle" / "packages"


def open_no_follow(path: Union[Path, str], mode: str = "r") -> Any:
    return open(
        path,
        mode=mode,
        opener=lambda path, flags: os.open(path, flags | os.O_NOFOLLOW),
    )


def copyfile_no_follow(src: Union[Path, str], dst: Union[Path, str]) -> None:
    fsrc = open_no_follow(src, "rb")
    fdst = open_no_follow(dst, "wb")
    shutil.copyfileobj(fsrc, fdst)


def add_packages(dir: Path, origin: str) -> None:
    for package_dir in dir.iterdir():
        if package_dir.name not in PACKAGES:
            try:
                build_depends = set(
                    path.strip()
                    for path in open_no_follow(package_dir / "build-depends.txt")
                )
            except FileNotFoundError:
                build_depends = set()
            try:
                depends = set(
                    path.strip() for path in open_no_follow(package_dir / "depends.txt")
                )
            except FileNotFoundError:
                depends = set()
            depends.add("auto")
            if (package_dir / "test.sh").exists():
                test = package_dir / "test.sh"
            else:
                test = None
            if (package_dir / "update.sh").exists():
                update = package_dir / "update.sh"
            else:
                update = None
            PACKAGES[package_dir.name] = {
                "build-depends": build_depends,
                "depends": depends,
                "dir": package_dir,
                "origin": origin,
                "update": update,
                "test": test,
            }


def transitive_depends(
    packages: Iterable[PackageName], *, build_depends: bool
) -> set[PackageName]:
    visited = set()

    def visit(p: PackageName) -> None:
        if p not in visited:
            visited.add(p)
            for q in PACKAGES[p]["depends"]:
                visit(q)
            if build_depends:
                for q in PACKAGES[p]["build-depends"]:
                    visit(q)

    for p in packages:
        visit(p)
    return visited


PACKAGES: dict[str, PackageSpec] = {}
USER_PACKAGE_DIR.mkdir(exist_ok=True, parents=True)
for dir in sorted(USER_PACKAGE_DIR.iterdir()):
    add_packages(dir, dir.name)
CODE_PACKAGE_DIR.mkdir(exist_ok=True, parents=True)
add_packages(CODE_PACKAGE_DIR, "built-in")

for package in transitive_depends(["auto"], build_depends=True):
    d = PACKAGES[package]["depends"]
    try:
        d.remove("auto")
    except ValueError:
        pass


def rmtree(path: Path) -> None:
    # This is a bit challenging due to two issues:
    #
    # 1. shutil.rmtree and `rm` won't remove the contents of read-only
    #    directories, such as Go's packages. See
    #    <https://github.com/golang/go/issues/27161>.
    #
    # 2. Docker might leave empty directories owned by root. Specifically, it
    #    seems to often leave one where a volume was mounted, like a Cubicle
    #    container's work directory within its home directory. These are
    #    removable but their permissions can't be altered by chmod.
    def onerror(_: Any, path: Any, exc_info: Any) -> None:
        err: BaseException = exc_info[1]
        if isinstance(err, PermissionError):
            path = Path(path)
            subprocess.run(["chmod", "-R", "u+rwX", path.parent], check=True)
            if path.is_dir():
                path.rmdir()
            else:
                path.unlink()
            return
        raise err

    shutil.rmtree(path, onerror=onerror)


def update_packages(packages: Iterable[PackageName]) -> None:
    now = time.time()
    todo = sorted(transitive_depends(packages, build_depends=True))
    done: set[PackageName] = set()
    while len(todo) > 0:
        later = []
        for key in todo:
            package = PACKAGES[key]
            if done.issuperset(package["depends"]) and done.issuperset(
                package["build-depends"]
            ):
                update_stale_package(key, now)
                done.add(key)
            else:
                later.append(key)
        if len(later) == len(todo):
            raise RuntimeError(
                f"Package dependencies are unsatisfiable for: {sorted(todo)}"
            )
        todo = later


def last_built(package: PackageName) -> float:
    try:
        return (PACKAGE_CACHE / f"{package}.tar").stat().st_mtime
    except FileNotFoundError:
        return 0


def update_stale_package(key: PackageName, now: float) -> None:
    package = PACKAGES[key]
    name = f"package-{key}"
    mtime = du(package["dir"])[2]

    if package["update"] is None:
        return

    work_dir = WORK_DIRS / name
    if not work_dir.exists():
        work_dir.mkdir(parents=True)
    built = last_built(key)
    if (
        mtime < built
        and now - built < 60 * 60 * 12
        and all(
            last_built(p) < built
            for p in package["build-depends"].union(package["depends"])
        )
    ):
        return
    update_package(key)


def update_package(key: PackageName) -> None:
    package = PACKAGES[key]
    name = f"package-{key}"
    print(f"Updating {key} package")
    tar_path = XDG_CACHE_HOME / "cubicle" / f"{name}.tar"
    subprocess.run(
        flatten(
            "tar",
            "-c",
            ("--directory", package["dir"]),
            ".",
            ("--transform", f"s/^\./{name}/"),
            ("-f", tar_path),
        ),
        stdout=subprocess.PIPE,
        check=True,
    )
    try:
        run(
            name,
            packages=package["build-depends"].union(package["depends"]),
            extra_seeds=[tar_path],
            init=(SCRIPT_PATH / "dev-init.sh"),
        )
    except subprocess.CalledProcessError as e:
        if not (PACKAGE_CACHE / f"{key}.tar").is_file():
            raise e
        print(
            f"WARNING: Failed to update package {key} (exit status {e.returncode}). Keeping stale version."
        )
        return
    finally:
        tar_path.unlink()

    PACKAGE_CACHE.mkdir(exist_ok=True, parents=True)
    if package["test"] is None:
        # We want to access `provides.tar` from the package build container.
        # However, that could potentially be a (malicious) symlink that points
        # to some sensitive file elsewhere on the host. This throws an
        # exception if `provides.tar` is a symlink.
        copyfile_no_follow(
            HOME_DIRS / name / "provides.tar",
            PACKAGE_CACHE / f"{key}.tar",
        )
    else:
        print(f"Testing {key} package")
        test_name = f"test-package-{key}"
        subprocess.run(
            flatten(
                "tar",
                "-c",
                "--anchored",
                ("--directory", package["dir"]),
                (
                    # dev-init.sh will run `update.sh` if it's present, but we
                    # don't want that
                    "--exclude",
                    "./update.sh",
                ),
                ".",
                ("--transform", f"s/^\./{test_name}/"),
                ("-f", tar_path),
            ),
            stdout=subprocess.PIPE,
            check=True,
        )

        # We want to access `provides.tar` from the package build container.
        # However, that could potentially be a (malicious) symlink that points
        # to some sensitive file elsewhere on the host. This throws an
        # exception if `provides.tar` is a symlink.
        copyfile_no_follow(
            HOME_DIRS / name / "provides.tar",
            PACKAGE_CACHE / f"{key}.testing.tar",
        )

        try:
            purge_environment(test_name, quiet=True)
            work_dir = WORK_DIRS / test_name
            work_dir.mkdir(parents=True)
            run(
                test_name,
                packages=package["depends"],
                extra_seeds=[tar_path, PACKAGE_CACHE / f"{key}.testing.tar"],
                init=(SCRIPT_PATH / "dev-init.sh"),
            )
            run(
                test_name,
                exec=["./test.sh"],
            )
            purge_environment(f"test-package-{key}")
        except subprocess.CalledProcessError as e:
            (PACKAGE_CACHE / f"{key}.testing.tar").unlink()
            if not (PACKAGE_CACHE / f"{key}.tar").is_file():
                raise e
            print(
                f"WARNING: Updated package {key} failed tests (exit status {e.returncode}). Keeping stale version."
            )
            return
        finally:
            tar_path.unlink()

        os.rename(
            PACKAGE_CACHE / f"{key}.testing.tar",
            PACKAGE_CACHE / f"{key}.tar",
        )


def enter_environment(name: str) -> None:
    if not (WORK_DIRS / name).exists():
        print(f"error: environment {name!s} does not exist")
        sys.exit(1)
    run(name)


def exec_environment(name: str, command: str, args: list[str]) -> None:
    if not (WORK_DIRS / name).exists():
        print(f"error: environment {name!s} does not exist")
        sys.exit(1)
    run(name, exec=([command] + args))


def du(path: Path) -> tuple[bool, int, int]:
    result = subprocess.run(
        ["du", "-cs", "--block-size=1", "--time", "--time-style=+%s", path],
        capture_output=True,
        check=False,  # ignore permissions errors
        encoding="utf-8",
    )
    m = re.search(
        r"^(?P<size>[^\t]+)\t(?P<mtime>[0-9]+)\ttotal$",
        result.stdout,
        re.MULTILINE,
    )
    if m is None:
        raise RuntimeError("Unexpected output from du")
    size = int(m.group("size"))
    mtime = int(m.group("mtime"))
    return (result.stderr != "", size, mtime)


def try_iterdir(path: Path) -> Iterable[Path]:
    try:
        yield from path.iterdir()
    except FileNotFoundError:
        pass


def rel_time(then: float, now: Optional[float] = None) -> str:
    if then == 0:
        return "N/A"
    if now is None:
        now = time.time()
    duration = now - then
    duration /= 60
    if duration < 59.5:
        return f"{duration:.0f} minutes"
    duration /= 60
    if duration < 23.5:
        return f"{duration:.0f} hours"
    duration /= 24
    return f"{duration:.0f} days"


def si_bytes(size: int) -> str:
    if size < 1_000:
        return f"{size} B"
    if size < 999_950:
        return f"{size/1e3:.1f} kB"
    if size < 999_950 * 1e3:
        return f"{size/1e6:.1f} MB"
    if size < 999_950 * 1e6:
        return f"{size/1e9:.1f} GB"
    return f"{size/1e12:.1f} TB"


def list_environments(format: str = "default") -> None:
    if format == "names":
        # fast path for shell completions
        for name in sorted([dir.name for dir in try_iterdir(WORK_DIRS)]):
            print(name)
        return

    now = time.time()
    envs: dict[str, Any] = {}
    for dir in try_iterdir(WORK_DIRS):
        env = envs.setdefault(dir.name, {})
        env["work_dir"] = str(dir)
        try:
            (
                env["work_dir_du_error"],
                env["work_dir_size"],
                env["work_dir_mtime"],
            ) = du(dir)
        except RuntimeError:
            pass

    for dir in try_iterdir(HOME_DIRS):
        env = envs.setdefault(dir.name, {})
        env["home_dir"] = str(dir)
        try:
            (
                env["home_dir_du_error"],
                env["home_dir_size"],
                env["home_dir_mtime"],
            ) = du(dir)
        except RuntimeError:
            pass

    if format == "json":
        print(json.dumps(envs, sort_keys=True, indent=4))
    else:
        nw = max([10] + [len(name) for name in envs])
        print(
            "{:<{nw}} | {:^24} | {:^24}".format(
                "",
                "home directory",
                "work directory",
                nw=nw,
            )
        )
        print(
            "{:<{nw}} | {:>10} {:>13} | {:>10} {:>13}".format(
                "name",
                "size",
                "modified",
                "size",
                "modified",
                nw=nw,
            )
        )
        print("{0:-<{nw}} + {0:-<10} {0:-<13} + {0:-<10} {0:-<13}".format("", nw=nw))
        for name in sorted(envs):
            env = envs[name]
            if "home_dir_size" in env:
                home_dir_size = si_bytes(env["home_dir_size"])
                if env["home_dir_du_error"]:
                    home_dir_size += "+"
            else:
                home_dir_size = "N/A"
            if "work_dir_size" in env:
                work_dir_size = si_bytes(env["work_dir_size"])
                if env["work_dir_du_error"]:
                    work_dir_size += "+"
            else:
                work_dir_size = "N/A"
            print(
                "{:<{nw}} | {:>10} {:>13} | {:>10} {:>13}".format(
                    name,
                    home_dir_size,
                    rel_time(env["home_dir_mtime"], now)
                    if "home_dir_mtime" in env
                    else "N/A",
                    work_dir_size,
                    rel_time(env["work_dir_mtime"], now)
                    if "work_dir_mtime" in env
                    else "N/A",
                    nw=nw,
                )
            )


def list_packages(format: str = "default") -> None:
    if format == "names":
        # fast path for shell completions
        for name in sorted([package for package in PACKAGES]):
            print(name)
        return

    now = time.time()
    packages: dict[PackageName, Any] = {}
    for name, p in PACKAGES.items():
        error, size, built = du(PACKAGE_CACHE / f"{name}.tar")
        packages[name] = {
            "build-depends": sorted(p["build-depends"]),
            "dir": str(p["dir"]),
            "depends": sorted(p["depends"]),
            "origin": p["origin"],
            "edited": du(p["dir"])[2],
            "built": built,
            "size": None if error else size,
        }

    if format == "json":
        print(json.dumps(packages, sort_keys=True, indent=4))
    else:
        nw = max([10] + [len(name) for name in packages])
        print(
            "{:<{nw}}  {:<8}  {:>10}  {:>13}  {:>13}".format(
                "name",
                "origin",
                "size",
                "built",
                "edited",
                nw=nw,
            )
        )
        print("{0:-<{nw}}  {0:-<8}  {0:-<10}  {0:-<13}  {0:-<13}".format("", nw=nw))
        for name in sorted(packages):
            package = packages[name]
            print(
                "{:<{nw}}  {:<8}  {:>10}  {:>13}  {:>13}".format(
                    name,
                    package["origin"],
                    "N/A" if package["size"] is None else si_bytes(package["size"]),
                    rel_time(package["built"], now),
                    rel_time(package["edited"], now),
                    nw=nw,
                )
            )


def new_environment(name: str, packages: list[PackageName] = ["default"]) -> None:
    work_dir = WORK_DIRS / name
    if work_dir.exists() or (HOME_DIRS / name).exists():
        print(
            f"error: environment {name!s} exists (did you mean '{SCRIPT_NAME} reset'?)"
        )
        sys.exit(1)
    update_packages(packages)
    work_dir.mkdir(parents=True)
    open_no_follow(work_dir / "packages.txt", "w").write(
        "\n".join(sorted(packages)) + "\n"
    )
    run(name, packages=packages, init=(SCRIPT_PATH / "dev-init.sh"))


def random_names() -> Iterable[str]:
    # 1. Prefer the EFF short word list. See https://www.eff.org/dice for more
    # info.
    words = None
    EFF_WORDLIST_PATH = XDG_CACHE_HOME / "cubicle" / "eff_short_wordlist_1.txt"
    try:
        words = open(EFF_WORDLIST_PATH).readlines()
    except FileNotFoundError:
        url = "https://www.eff.org/files/2016/09/08/eff_short_wordlist_1.txt"
        try:
            contents = urllib.request.urlopen(url).read().decode("utf-8")
        except (urllib.request.HTTPError, urllib.request.URLError) as e:  # type: ignore
            print(f"Warning: failed to download EFF short wordlist from {url}: {e}")
        else:
            EFF_WORDLIST_PATH.parent.mkdir(exist_ok=True, parents=True)
            open(EFF_WORDLIST_PATH, "w").write(contents)
            words = contents.split("\n")
    if words is not None:
        for _ in range(200):
            word = random.choice(words).split()[1]
            if len(word) <= 10 and word.islower() and word.isalpha():
                yield word

    # 2. /usr/share/dict/words
    try:
        words = open("/usr/share/dict/words").readlines()
    except FileNotFoundError:
        pass
    else:
        for _ in range(200):
            word = random.choice(words).strip()
            if len(word) <= 6 and word.islower() and word.isalpha():
                yield word

    # 3. Random 6 letters
    for _ in range(20):
        yield "".join(random.choices("abcdefghijklmnopqrstuvwxyz", k=6))

    # 4. Random 32 letters
    yield "".join(random.choices("abcdefghijklmnopqrstuvwxyz", k=32))


def create_enter_tmp_environment(packages: list[PackageName] = ["default"]) -> None:
    for name in random_names():
        name = f"tmp-{name}"
        work_dir = WORK_DIRS / name
        if not work_dir.exists() and not (HOME_DIRS / name).exists():
            update_packages(packages)
            work_dir.mkdir(parents=True)
            open_no_follow(work_dir / "packages.txt", "w").write(
                "\n".join(sorted(packages)) + "\n"
            )
            run(name, packages=packages, init=(SCRIPT_PATH / "dev-init.sh"))
            run(name)
            return
    raise RuntimeError("failed to generate random environment name")


def purge_environment(name: str, *, quiet: bool = False) -> None:
    host_work = WORK_DIRS / name
    host_home = HOME_DIRS / name
    if not host_work.exists() and not host_home.exists():
        if not quiet:
            print(f"warning: environment {name} does not exist (nothing to purge)")
        return
    RUNNER.kill(name)
    if host_work.exists():
        rmtree(host_work)
    if host_home.exists():
        rmtree(host_home)


def reset_environment(
    name: str,
    packages: Optional[Iterable[PackageName]] = None,
    clean: bool = False,
) -> None:
    work_dir = WORK_DIRS / name
    if not work_dir.exists():
        print(
            f"error: environment {name!s} does not exist (did you mean '{SCRIPT_NAME} new'?)"
        )
        sys.exit(1)
    host_home = HOME_DIRS / name
    if host_home.exists():
        RUNNER.kill(name)
        rmtree(host_home)
    if clean:
        return

    if packages is None:
        try:
            packages = {
                p.strip()
                for p in open_no_follow(work_dir / "packages.txt")
                if p.strip() != ""
            }
        except FileNotFoundError:
            packages = set()
    m = re.match("^package-(.*)$", name)
    if m is None:
        update_packages(packages)
        open_no_follow(work_dir / "packages.txt", "w").write(
            "\n".join(sorted(packages)) + "\n"
        )
        run(name, packages=packages, init=(SCRIPT_PATH / "dev-init.sh"))
    else:
        key = m.group(1)
        package = PACKAGES[key]
        packages = package["build-depends"].union(package["depends"], packages)
        update_packages(packages)
        update_package(key)
        open_no_follow(work_dir / "packages.txt", "w").write(
            "\n".join(sorted(packages)) + "\n"
        )
        run(name, packages=packages, init=(SCRIPT_PATH / "dev-init.sh"))


def flatten(
    *l: Sequence[Union[T, Sequence[Union[T, Sequence[T]]]]],
) -> Sequence[T]:
    def gen(l: Any) -> Iterable[T]:
        for x in l:
            if isinstance(x, (list, tuple)):
                yield from gen(x)
            else:
                yield x

    return list(gen(l))


def ro_bind_try(
    a: Union[str, Path], b: Optional[Union[str, Path]] = None
) -> tuple[str, Union[str, Path], Union[str, Path]]:
    if b is None:
        return ("--ro-bind-try", a, a)
    return ("--ro-bind-try", a, b)


def packages_to_seeds(packages: Iterable[PackageName]) -> list[Path]:
    args = []
    for package in sorted(transitive_depends(packages, build_depends=False)):
        provides = PACKAGE_CACHE / f"{package}.tar"
        if provides.is_file():
            args.append(provides)
    return args


def assert_some(x: Optional[T]) -> T:
    assert x is not None
    return x


def run(
    name: str,
    packages: Iterable[PackageName] = [],
    extra_seeds: list[Path] = [],
    init: Union[Literal[False], Path] = False,
    exec: Union[Literal[False], list[str]] = False,
) -> None:
    # print(f'run({name}, packages={packages}, extra_seed={extra_seed}, init={init}, exec={exec}')
    host_home = HOME_DIRS / name
    host_work = WORK_DIRS / name

    try:
        host_home.mkdir(parents=True)
    except FileExistsError:
        pass

    seeds = packages_to_seeds(packages) + extra_seeds
    RUNNER.run(
        name=name,
        host_home=host_home,
        host_work=host_work,
        seeds=seeds,
        init=init,
        exec=exec,
    )


class Runner(ABC):
    @abstractmethod
    def kill(
        self,
        name: str,
    ) -> None:
        pass

    @abstractmethod
    def run(
        self,
        *,
        name: str,
        host_home: Path,
        host_work: Path,
        seeds: list[Path],
        init: Union[Literal[False], Path],
        exec: Union[Literal[False], list[str]],
    ) -> None:
        pass


class Bubblewrap(Runner):
    def kill(self, name: str) -> None:
        pass

    def run(
        self,
        *,
        name: str,
        host_home: Path,
        host_work: Path,
        seeds: list[Path],
        init: Union[Literal[False], Path],
        exec: Union[Literal[False], list[str]],
    ) -> None:
        seed: Optional[subprocess.Popen[bytes]] = None
        if seeds:
            print("Packing seed tarball")
            seed = subprocess.Popen(
                ["pv", "-i", "0.1", *seeds],
                stdout=subprocess.PIPE,
            )

        env: dict[str, str | Path] = {
            "PATH": f"{HOME}/bin:/bin:/sbin",
            "SANDBOX": name,
            "TMPDIR": HOME / "tmp",
        }
        for var in ["DISPLAY", "HOME", "SHELL", "TERM"]:
            if var in os.environ:
                env[var] = os.environ[var]

        seccomp = open(SCRIPT_PATH / "seccomp.bpf")
        bwrap = subprocess.Popen(
            flatten(
                "bwrap",
                "--die-with-parent",
                "--unshare-cgroup",
                "--unshare-ipc",
                "--unshare-pid",
                "--unshare-uts",
                ("--hostname", f"{name}.{HOSTNAME}"),
                ("--symlink", "/usr/bin", "/bin"),
                ("--dev", "/dev"),
                (ro_bind_try(init, "/dev/shm/init.sh") if init else []),
                (
                    []
                    if seed is None
                    else [
                        "--file",
                        str(assert_some(seed.stdout).fileno()),
                        "/dev/shm/seed.tar",
                    ]
                ),
                ro_bind_try("/etc"),
                ("--bind", host_home, HOME),
                ("--dir", HOME / ".dev-init"),
                ("--dir", HOME / "bin"),
                ("--dir", HOME / "opt"),
                ("--dir", HOME / "tmp"),
                ("--bind", host_work, HOME / name),
                ("--symlink", "/usr/lib", "/lib"),
                ("--symlink", "/usr/lib64", "/lib64"),
                ro_bind_try("/opt"),
                ("--proc", "/proc"),
                ("--symlink", "/usr/sbin", "/sbin"),
                ("--tmpfs", "/tmp"),
                ro_bind_try("/usr"),
                ro_bind_try("/var/lib/apt/lists/"),
                ro_bind_try("/var/lib/dpkg/"),
                ("--seccomp", str(seccomp.fileno())),
                ("--chdir", HOME / name),
                "--",
                os.environ["SHELL"],
                "-l",
                (
                    ["-c", "/dev/shm/init.sh"]
                    if init
                    else ["-c", shlex.join(exec)]
                    if exec
                    else []
                ),
            ),
            env=env,
            pass_fds=[
                *([] if seed is None else [assert_some(seed.stdout).fileno()]),
                seccomp.fileno(),
            ],
        )

        if seed is not None:
            assert_some(seed.stdout).close()  # so tar receives SIGPIPE

        if bwrap.wait() != 0:
            raise subprocess.CalledProcessError(bwrap.returncode, "bwrap")

        if seed is not None:
            seed.wait()


class Docker(Runner):
    def kill(self, name: str) -> None:
        if self.is_running(name):
            subprocess.run(
                ["docker", "kill", name],
                check=True,
                stdout=subprocess.DEVNULL,
            )

    def is_running(self, name: str) -> bool:
        result = subprocess.run(
            ["docker", "inspect", "--type", "container", name],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        return result.returncode == 0

    def base_mtime(self) -> int:
        result = subprocess.run(
            [
                "docker",
                "inspect",
                "--type",
                "image",
                "--format",
                "{{ $.Metadata.LastTagTime.Unix }}",
                "cubicle-base",
            ],
            capture_output=True,
        )
        if result.returncode == 0:
            return int(result.stdout)
        else:
            return 0

    def build_base(self) -> None:
        base_mtime = self.base_mtime()
        if (
            time.time() - base_mtime < 60 * 60 * 12
            and (SCRIPT_PATH / "Dockerfile.in").stat().st_mtime < base_mtime
        ):
            return
        dockerfile = (
            open(SCRIPT_PATH / "Dockerfile.in")
            .read()
            .replace("@@TIMEZONE@@", open("/etc/timezone").read().strip())
            .replace("@@USER@@", os.environ["USER"])
        )
        subprocess.run(
            ["docker", "build", "--tag", "cubicle-base", "-"],
            input=dockerfile.encode(),
            check=True,
        )

    def spawn(
        self,
        *,
        name: str,
        host_home: Path,
        host_work: Path,
    ) -> None:
        seccomp_json = SCRIPT_PATH / "seccomp.json"
        subprocess.run(
            flatten(
                "docker",
                "run",
                "--detach",
                ("--env", f"SANDBOX={name}"),
                ("--hostname", f"{name}.{HOSTNAME}"),
                "--init",
                ("--name", name),
                "--rm",
                (
                    ("--security-opt", f"seccomp={ seccomp_json }")
                    if seccomp_json.is_file()
                    else []
                ),
                (
                    # The default `/dev/shm` is limited to only 64 MiB under
                    # Docker (v20.10.5), which causes many crashes in Chromium
                    # and Electron-based programs. See
                    # <https://github.com/ongardie/cubicle/issues/3>.
                    "--shm-size",
                    str(1_000_000_000),
                ),
                ("--user", os.environ["USER"]),
                ("--volume", "/tmp/.X11-unix:/tmp/.X11-unix:ro"),
                ("--volume", f"{host_home}:{HOME}"),
                ("--volume", f"{host_work}:{HOME / name}"),
                ("--workdir", HOME / name),
                "cubicle-base",
                ("sleep", "90d"),
            ),
            check=True,
            stdout=subprocess.DEVNULL,
        )

    def run(
        self,
        *,
        name: str,
        host_home: Path,
        host_work: Path,
        seeds: list[Path],
        init: Union[Literal[False], Path],
        exec: Union[Literal[False], list[str]],
    ) -> None:
        if not self.is_running(name):
            self.build_base()
            self.spawn(name=name, host_home=host_home, host_work=host_work)
        if seeds:
            print("Copying/extracting seed tarball")
            # Use pv from inside the container since it may not be installed on
            # the host. Since it's reading from a stream, it needs to know the
            # total size to display a good progress bar.
            size = sum(du(s)[1] for s in seeds)
            seed = subprocess.Popen(
                ["cat", *seeds],
                stdout=subprocess.PIPE,
            )
            subprocess.run(
                [
                    "docker",
                    "exec",
                    "--interactive",
                    name,
                    "sh",
                    "-c",
                    (
                        f"pv --interval 0.1 --force --size {size} | "
                        + "tar --ignore-zero --directory ~ --extract"
                    ),
                ],
                check=True,
                stdin=seed.stdout,
            )

        if init:
            subprocess.run(
                ["docker", "cp", "--archive", init, f"{name}:/cubicle-init.sh"],
                check=True,
            )

        subprocess.run(
            flatten(
                "docker",
                "exec",
                ("--env", "DISPLAY"),
                (
                    # The debian:11 image hasn't gone through usrmerge, so
                    # /usr/bin and /bin are distinct there.
                    "--env",
                    f"PATH={HOME}/bin:/bin:/sbin:/usr/bin:/usr/sbin",
                ),
                ("--env", "SHELL"),
                ("--env", "TERM"),
                ("--env", "USER"),
                "--interactive",
                "--tty",
                name,
                os.environ["SHELL"],
                "-l",
                (
                    ["-c", "/cubicle-init.sh"]
                    if init
                    else ["-c", shlex.join(exec)]
                    if exec
                    else []
                ),
            ),
            check=True,
        )


def package_list(packages: str) -> set[PackageName]:
    if packages == "none":
        return set()
    package_set = {p.strip() for p in packages.split(",") if p.strip() != ""}.union(
        {"auto"}
    )
    for package in package_set:
        if package not in PACKAGES:
            options = ", ".join([repr(s) for s in PACKAGES])
            raise argparse.ArgumentTypeError(
                f"invalid package {package!r} (use 'none' or comma-separated list from {options})"
            )
    return package_set


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Manage sandboxed environments", allow_abbrev=False
    )
    subparsers = parser.add_subparsers(title="commands", dest="command", metavar=None)

    parser_enter = subparsers.add_parser(
        "enter", help="Run a shell in an existing environment", allow_abbrev=False
    )
    parser_enter.add_argument("name", help="Environment name")

    parser_exec = subparsers.add_parser(
        "exec", help="Run a command in an existing environment", allow_abbrev=False
    )
    parser_exec.add_argument("name", help="Environment name")
    parser_exec.add_argument("exec", metavar="command", help="Command to run")
    parser_exec.add_argument(
        "args",
        nargs="*",
        help='Arguments to command (use "--" before command to disambiguate)',
    )

    parser_help = subparsers.add_parser(
        "help", help="Show help information", allow_abbrev=False
    )

    parser_list = subparsers.add_parser(
        "list", help="Show existing environments", allow_abbrev=False
    )
    parser_list.add_argument(
        "--format", choices=["default", "json", "names"], help="Set output format"
    )

    parser_new = subparsers.add_parser(
        "new", help="Create a new environment", allow_abbrev=False
    )
    parser_new.add_argument(
        "--enter", action="store_true", help="Run a shell in new environment"
    )
    parser_new.add_argument(
        "--packages",
        type=package_list,
        default="default",
        help="Comma-separated names of packages to inject into home directory",
    )
    parser_new.add_argument("name", help="Environment name")

    parser_packages = subparsers.add_parser(
        "packages", help="Show available packages", allow_abbrev=False
    )
    parser_packages.add_argument(
        "--format", choices=["default", "json", "names"], help="Set output format"
    )

    parser_purge = subparsers.add_parser(
        "purge", help="Delete an environment and its work directory", allow_abbrev=False
    )
    parser_purge.add_argument("name", nargs="+", help="Environment name(s)")

    parser_reset = subparsers.add_parser(
        "reset",
        help="Recreate an environment (keeping its work directory)",
        allow_abbrev=False,
    )
    parser_reset.add_argument(
        "--clean",
        action="store_true",
        help="Remove home directory and do not recreate it",
    )
    parser_reset.add_argument(
        "--packages",
        type=package_list,
        help="Comma-separated names of packages to inject into home directory",
    )
    parser_reset.add_argument("name", nargs="+", help="Environment name(s)")

    parser_tmp = subparsers.add_parser(
        "tmp", help="Create and enter a new temporary environment", allow_abbrev=False
    )
    parser_tmp.add_argument(
        "--packages",
        type=package_list,
        default="default",
        help="Comma-separated names of packages to inject into home directory",
    )

    # TODO: rename

    args = parser.parse_args()
    if args.command is None:
        parser.print_help(sys.stderr)
        sys.exit(1)
    if args.command == "help":
        parser.print_help(sys.stderr)
        sys.exit(0)
    return args


if __name__ == "__main__":
    runners = "'bubblewrap' or 'docker'"
    try:
        runner = open(SCRIPT_PATH / ".RUNNER").read().strip()
    except FileNotFoundError:
        print(f"Runner should be set to {runners}")
        raise
    RUNNER: Runner
    if runner == "bubblewrap":
        RUNNER = Bubblewrap()
    elif runner == "docker":
        RUNNER = Docker()
    else:
        raise RuntimeError(
            f"Unknown runner in {SCRIPT_PATH / '.RUNNER'}: {runner!r}, expected {runners}"
        )

    args = parse_args()
    if args.command == "enter":
        enter_environment(name=args.name)
    elif args.command == "exec":
        exec_environment(name=args.name, command=args.exec, args=args.args)
    elif args.command == "list":
        list_environments(format=args.format)
    elif args.command == "new":
        new_environment(name=args.name, packages=args.packages)
        if args.enter:
            enter_environment(name=args.name)
    elif args.command == "packages":
        list_packages(format=args.format)
    elif args.command == "purge":
        for name in args.name:
            purge_environment(name=name)
    elif args.command == "reset":
        for name in args.name:
            reset_environment(name=name, packages=args.packages, clean=args.clean)
    elif args.command == "tmp":
        create_enter_tmp_environment(packages=args.packages)
    else:
        raise RuntimeError(f"unknown command: {args}")
