// Proxycache CLI - Management tool for the proxy server
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"sort"
	"strconv"
	"strings"
	"syscall"
	"time"

	toml "github.com/pelletier/go-toml/v2"
)

const (
	reset  = "\033[0m"
	bold   = "\033[1m"
	red    = "\033[31m"
	green  = "\033[32m"
	yellow = "\033[33m"
	cyan   = "\033[36m"
	dim    = "\033[90m"
	sep    = "──────────────────────────────────────────"
)

var (
	addr   = "127.0.0.1:9090"
	apiKey = ""
	client = &http.Client{Timeout: 5 * time.Second}
)

func main() {
	args := parseFlags()
	if len(args) > 0 {
		runCmd(strings.Join(args, " "))
		if webRunning {
			select {}
		}
		return
	}
	repl()
}

func parseFlags() []string {
	var rest []string
	a := os.Args[1:]
	for i := 0; i < len(a); i++ {
		if a[i] == "--addr" && i+1 < len(a) {
			addr = a[i+1]
			i++
		} else if a[i] == "--key" && i+1 < len(a) {
			apiKey = a[i+1]
			i++
		} else {
			rest = append(rest, a[i])
		}
	}
	if apiKey == "" {
		loadAPIKeyFromConfig()
	}
	return rest
}

func loadAPIKeyFromConfig() {
	cfg, err := loadConfigTOML()
	if err != nil {
		return
	}
	mods := getModules(cfg)
	if mods == nil {
		return
	}
	admin, ok := mods["admin_api"].(map[string]interface{})
	if !ok {
		return
	}
	if key, ok := admin["api_key"].(string); ok && key != "" {
		apiKey = key
	}
}

func repl() {
	fmt.Printf("\n%s%sProxycache CLI%s\n", bold, cyan, reset)
	fmt.Printf("%s%s%s\n", dim, sep, reset)
	fmt.Printf("Admin: %s%s%s  |  Type %shelp%s for commands\n\n", cyan, addr, reset, cyan, reset)

	sc := bufio.NewScanner(os.Stdin)
	for {
		fmt.Printf("%s❯%s ", cyan, reset)
		if !sc.Scan() {
			break
		}
		line := strings.TrimSpace(sc.Text())
		if line == "" {
			continue
		}
		runCmd(line)
		fmt.Println()
	}
}

func runCmd(input string) {
	parts := strings.Fields(input)
	if len(parts) == 0 {
		return
	}
	cmd := parts[0]
	args := parts[1:]

	switch cmd {
	case "status":
		doStatus()
	case "stop":
		doStop()
	case "reload":
		doReload()
	case "ping":
		doPing()
	case "logs":
		doLogs()
	case "compile", "build":
		doCompile()
	case "run", "start":
		doRun()
	case "ls", "modules":
		doListModules()
	case "toggle":
		if len(args) < 1 {
			fmt.Printf("  %sUsage: toggle <module|web>%s\n", yellow, reset)
		} else if args[0] == "web" {
			toggleWeb()
		} else {
			doToggle(args[0])
		}
	case "edit":
		if len(args) < 1 {
			fmt.Printf("  %sUsage: edit <module|server>%s\n", yellow, reset)
		} else {
			doEditSection(args[0])
		}
	case "web":
		doWeb()
	case "help":
		printHelp()
	case "clear", "cls":
		fmt.Print("\033[H\033[2J")
	case "exit", "quit":
		os.Exit(0)
	default:
		fmt.Printf("  %s✗ Unknown: %s%s\n", red, cmd, reset)
	}
}

func apiGet(path string) {
	req, _ := http.NewRequest("GET", fmt.Sprintf("http://%s%s", addr, path), nil)
	if apiKey != "" {
		req.Header.Set("X-API-Key", apiKey)
	}
	resp, err := client.Do(req)
	if err != nil {
		fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	printJSON(body)
}

func apiPost(path string) {
	req, _ := http.NewRequest("POST", fmt.Sprintf("http://%s%s", addr, path), nil)
	if apiKey != "" {
		req.Header.Set("X-API-Key", apiKey)
	}
	resp, err := client.Do(req)
	if err != nil {
		if path == "/stop" || path == "/reload" {
			action := strings.TrimPrefix(path, "/")
			fmt.Printf("  %s✓%s %s signal sent\n", green, reset, action)
			return
		}
		fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	printJSON(body)
}

func doPing() {
	start := time.Now()
	resp, err := client.Get(fmt.Sprintf("http://%s/ping", addr))
	elapsed := time.Since(start)
	if err != nil {
		fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
		return
	}
	resp.Body.Close()
	fmt.Printf("  %s✓ pong%s %s(%s)%s\n", green, reset, dim, elapsed.Round(time.Millisecond), reset)
}

func connErr(err error) string {
	s := err.Error()
	if strings.Contains(s, "refused") || strings.Contains(s, "No connection") || strings.Contains(s, "target machine actively refused") {
		return "proxy not running"
	}
	return s
}

func printJSON(data []byte) {
	var obj map[string]interface{}
	if err := json.Unmarshal(data, &obj); err != nil {
		fmt.Println(string(data))
		return
	}
	keys := make([]string, 0, len(obj))
	for k := range obj {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	for _, k := range keys {
		v := obj[k]
		switch val := v.(type) {
		case float64:
			if val == float64(int64(val)) {
				fmt.Printf("  %s%-16s%s %d\n", cyan, k, reset, int64(val))
			} else {
				fmt.Printf("  %s%-16s%s %g\n", cyan, k, reset, val)
			}
		default:
			fmt.Printf("  %s%-16s%s %v\n", cyan, k, reset, v)
		}
	}
}

func projectRoot() string {
	dir, _ := os.Getwd()
	for {
		if _, err := os.Stat(filepath.Join(dir, "Cargo.toml")); err == nil {
			return dir
		}
		parent := filepath.Dir(dir)
		if parent == dir {
			break
		}
		dir = parent
	}
	d, _ := os.Getwd()
	return d
}

func doCompile() {
	root := projectRoot()

	if !compileRust() {
		return
	}
	fmt.Printf("  %sCompiling CLI...%s\n", yellow, reset)
	cliDir := filepath.Join(root, "cli")
	cmd := exec.Command("go", "build", "-o", "proxycache-cli.exe", ".")
	cmd.Dir = cliDir
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		fmt.Printf("  %s✗ CLI build failed%s\n", red, reset)
		return
	}
	fmt.Printf("  %s✓ CLI build successful%s\n\n", green, reset)

	fmt.Printf("  %sRestarting CLI...%s\n\n", yellow, reset)
	time.Sleep(200 * time.Millisecond)

	cliBin := filepath.Join(cliDir, "proxycache-cli.exe")
	cmd = exec.Command(cliBin)
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	cmd.Dir = root
	_ = cmd.Run()
	os.Exit(0)
}

func doRun() {
	root := projectRoot()
	pidFile := filepath.Join(root, ".proxycache.pid")

	if pid, err := readPID(pidFile); err == nil {
		if isProcessRunning(pid) {
			fmt.Printf("  %s! Proxy already running%s (pid %d)\n", yellow, reset, pid)
			return
		}
	}

	bin := filepath.Join(root, binaryPath())
	if _, err := os.Stat(bin); err != nil {
		fmt.Printf("  %s✗ Binary not found. Run 'compile' first.%s\n", red, reset)
		return
	}

	logOut, err := os.Create(filepath.Join(root, ".proxycache.log"))
	if err != nil {
		fmt.Printf("  %s✗ Can't create log: %s%s\n", red, err, reset)
		return
	}
	logErr, _ := os.Create(filepath.Join(root, ".proxycache.err"))

	cmd := exec.Command(bin)
	cmd.Dir = root
	cmd.Stdout = logOut
	cmd.Stderr = logErr
	cmd.SysProcAttr = &syscall.SysProcAttr{
		CreationFlags: syscall.CREATE_NEW_PROCESS_GROUP | 0x00000008, // DETACHED_PROCESS
	}

	if err := cmd.Start(); err != nil {
		fmt.Printf("  %s✗ %s%s\n", red, err, reset)
		logOut.Close()
		if logErr != nil {
			logErr.Close()
		}
		return
	}

	logOut.Close()
	if logErr != nil {
		logErr.Close()
	}

	pid := cmd.Process.Pid
	if err := writePID(pidFile, pid); err != nil {
		fmt.Printf("  %s⚠ Started but couldn't write PID: %s%s\n", yellow, err, reset)
	}

	cmd.Process.Release()

	fmt.Printf("  %s✓ Proxy started%s (pid %d)\n", green, reset, pid)
	fmt.Printf("  %sLogs:%s .proxycache.log, .proxycache.err\n", dim, reset)
}

func doStatus() {
	root := projectRoot()
	pidFile := filepath.Join(root, ".proxycache.pid")

	pid, pidErr := readPID(pidFile)
	running := pidErr == nil && isProcessRunning(pid)

	resp, apiErr := adminRequest("GET", "/status")

	if running {
		fmt.Printf("  %s✓ Process running%s (pid %d)\n", green, reset, pid)
	} else {
		fmt.Printf("  %s✗ Process not running%s\n", red, reset)
		if pidErr == nil {
			os.Remove(pidFile)
		}
	}

	if apiErr == nil {
		defer resp.Body.Close()
		body, _ := io.ReadAll(resp.Body)
		fmt.Printf("  %s✓ API responding%s\n", green, reset)
		printJSON(body)
	} else {
		fmt.Printf("  %s✗ API not responding%s\n", red, reset)
	}
}

func doStop() {
	root := projectRoot()
	pidFile := filepath.Join(root, ".proxycache.pid")

	resp, err := adminRequest("POST", "/stop")
	if err == nil {
		resp.Body.Close()
		fmt.Printf("  %s✓ Stop signal sent%s\n", green, reset)
		time.Sleep(500 * time.Millisecond)
	}

	if pid, err := readPID(pidFile); err == nil {
		if isProcessRunning(pid) {
			if killProcess(pid) {
				fmt.Printf("  %s✓ Process killed%s (pid %d)\n", yellow, reset, pid)
			}
		}
		os.Remove(pidFile)
	}
}

func doReload() {
	fmt.Printf("  %s● Stopping...%s\n", yellow, reset)
	doStop()
	time.Sleep(300 * time.Millisecond)
	fmt.Printf("  %s● Compiling...%s\n", yellow, reset)
	if !compileRust() {
		return
	}
	fmt.Printf("  %s● Starting...%s\n", yellow, reset)
	doRun()
}

func readPID(path string) (int, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	return strconv.Atoi(strings.TrimSpace(string(data)))
}

func writePID(path string, pid int) error {
	return os.WriteFile(path, []byte(strconv.Itoa(pid)), 0644)
}

func isProcessRunning(pid int) bool {
	out, err := exec.Command("tasklist", "/FI", fmt.Sprintf("PID eq %d", pid), "/NH").Output()
	if err != nil {
		return false
	}
	return strings.Contains(string(out), strconv.Itoa(pid))
}

func killProcess(pid int) bool {
	proc, err := os.FindProcess(pid)
	if err != nil {
		return false
	}
	return proc.Kill() == nil
}

func doLogs() {
	root := projectRoot()
	logPath := filepath.Join(root, ".proxycache.log")

	data, err := os.ReadFile(logPath)
	if err != nil {
		fmt.Printf("  %s✗ Can't read logs: %s%s\n", red, err, reset)
		return
	}

	lines := strings.Split(string(data), "\n")
	start := len(lines) - 50
	if start < 0 {
		start = 0
	}

	fmt.Printf("  %sLast 50 lines of .proxycache.log:%s\n", dim, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	for _, line := range lines[start:] {
		if line != "" {
			fmt.Println(line)
		}
	}
}

func configPath() string {
	return filepath.Join(projectRoot(), "config.toml")
}

func loadConfigTOML() (map[string]interface{}, error) {
	data, err := os.ReadFile(configPath())
	if err != nil {
		return nil, err
	}
	var cfg map[string]interface{}
	if err := toml.Unmarshal(data, &cfg); err != nil {
		return nil, err
	}
	return cfg, nil
}

func saveConfigTOML(cfg map[string]interface{}) error {
	data, err := toml.Marshal(cfg)
	if err != nil {
		return err
	}
	return os.WriteFile(configPath(), data, 0644)
}

func getModules(cfg map[string]interface{}) map[string]interface{} {
	mods, ok := cfg["modules"]
	if !ok {
		return nil
	}
	m, ok := mods.(map[string]interface{})
	if !ok {
		return nil
	}
	return m
}

func doListModules() {
	cfg, err := loadConfigTOML()
	if err != nil {
		fmt.Printf("  %s✗ Can't read config: %s%s\n", red, err, reset)
		return
	}

	fmt.Printf("  %s%-20s %s%s\n", dim, "NAME", "STATUS", reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)

	if _, ok := cfg["server"].(map[string]interface{}); ok {
		fmt.Printf("  %-20s %s%-8s%s\n", "server", cyan, "core", reset)
	}

	mods := getModules(cfg)
	if mods == nil {
		return
	}

	names := make([]string, 0, len(mods))
	for k := range mods {
		names = append(names, k)
	}
	sort.Strings(names)

	for _, name := range names {
		mod, ok := mods[name].(map[string]interface{})
		if !ok {
			continue
		}

		enabled := false
		if e, ok := mod["enabled"]; ok {
			if b, ok := e.(bool); ok {
				enabled = b
			}
		}

		var statusIcon, statusColor string
		if enabled {
			statusIcon = "✓ on"
			statusColor = green
		} else {
			statusIcon = "✗ off"
			statusColor = red
		}

		fmt.Printf("  %-20s %s%-8s%s\n", name, statusColor, statusIcon, reset)
	}

	if isWebEnabled() {
		fmt.Printf("  %-20s %s%-8s%s\n", "web", green, "✓ on", reset)
	} else {
		fmt.Printf("  %-20s %s%-8s%s\n", "web", red, "✗ off", reset)
	}
}

func doToggle(name string) {
	if name == "server" {
		fmt.Printf("  %s✗ Can't toggle server, use 'edit server'%s\n", red, reset)
		return
	}
	cfg, err := loadConfigTOML()
	if err != nil {
		fmt.Printf("  %s✗ Can't read config: %s%s\n", red, err, reset)
		return
	}
	mods := getModules(cfg)
	if mods == nil {
		fmt.Printf("  %s✗ No modules section in config%s\n", red, reset)
		return
	}

	mod, ok := mods[name].(map[string]interface{})
	if !ok {
		fmt.Printf("  %s✗ Module '%s' not found%s\n", red, name, reset)
		fmt.Printf("  %sTip: use 'ls' to see available modules%s\n", dim, reset)
		return
	}

	enabled := false
	if e, ok := mod["enabled"]; ok {
		if b, ok := e.(bool); ok {
			enabled = b
		}
	}

	mod["enabled"] = !enabled
	mods[name] = mod
	cfg["modules"] = mods

	if err := saveConfigTOML(cfg); err != nil {
		fmt.Printf("  %s✗ Can't save config: %s%s\n", red, err, reset)
		return
	}

	if !enabled {
		fmt.Printf("  %s✓ %s enabled%s\n", green, name, reset)
	} else {
		fmt.Printf("  %s✗ %s disabled%s\n", yellow, name, reset)
	}
	fmt.Printf("  %sRun 'reload' to apply changes%s\n", dim, reset)
}

func doEditSection(name string) {
	cfg, err := loadConfigTOML()
	if err != nil {
		fmt.Printf("  %s✗ Can't read config: %s%s\n", red, err, reset)
		return
	}

	var section map[string]interface{}
	var sectionLabel string

	if name == "server" {
		s, ok := cfg["server"].(map[string]interface{})
		if !ok {
			fmt.Printf("  %s✗ No server section in config%s\n", red, reset)
			return
		}
		section = s
		sectionLabel = "[server]"
	} else {
		mods := getModules(cfg)
		if mods == nil {
			fmt.Printf("  %s✗ No modules section in config%s\n", red, reset)
			return
		}
		m, ok := mods[name].(map[string]interface{})
		if !ok {
			fmt.Printf("  %s✗ '%s' not found%s\n", red, name, reset)
			fmt.Printf("  %sTip: use 'ls' to see available entries%s\n", dim, reset)
			return
		}
		section = m
		sectionLabel = fmt.Sprintf("[modules.%s]", name)
	}

	keys := make([]string, 0, len(section))
	for k := range section {
		keys = append(keys, k)
	}
	sort.Strings(keys)

	fmt.Printf("  %s%s%s%s\n", bold, cyan, sectionLabel, reset)
	for _, k := range keys {
		fmt.Printf("    %s%-20s%s = %v\n", cyan, k, reset, section[k])
	}
	fmt.Printf("\n  %sEdit key=value (empty line to finish):%s\n", dim, reset)

	sc := bufio.NewScanner(os.Stdin)
	changed := false
	for {
		fmt.Printf("  %s→%s ", yellow, reset)
		if !sc.Scan() {
			break
		}
		line := strings.TrimSpace(sc.Text())
		if line == "" {
			break
		}

		eqIdx := strings.Index(line, "=")
		if eqIdx < 0 {
			fmt.Printf("    %s✗ Format: key=value%s\n", red, reset)
			continue
		}

		key := strings.TrimSpace(line[:eqIdx])
		valStr := strings.TrimSpace(line[eqIdx+1:])

		if _, exists := section[key]; !exists {
			fmt.Printf("    %s+ Adding new key '%s'%s\n", yellow, key, reset)
		}

		section[key] = parseValue(valStr)
		changed = true
		fmt.Printf("    %s✓ %s = %v%s\n", green, key, section[key], reset)
	}

	if !changed {
		fmt.Printf("  %sNo changes%s\n", dim, reset)
		return
	}

	if name == "server" {
		cfg["server"] = section
	} else {
		mods := getModules(cfg)
		mods[name] = section
		cfg["modules"] = mods
	}

	if err := saveConfigTOML(cfg); err != nil {
		fmt.Printf("  %s✗ Can't save config: %s%s\n", red, err, reset)
		return
	}
	fmt.Printf("  %s✓ Saved%s. Run 'reload' to apply changes\n", green, reset)
}

func parseValue(s string) interface{} {
	if s == "true" {
		return true
	}
	if s == "false" {
		return false
	}
	if n, err := strconv.ParseInt(s, 10, 64); err == nil {
		return n
	}
	if f, err := strconv.ParseFloat(s, 64); err == nil {
		return f
	}
	if strings.HasPrefix(s, "[") && strings.HasSuffix(s, "]") {
		inner := strings.TrimSpace(s[1 : len(s)-1])
		if inner == "" {
			return []interface{}{}
		}
		parts := strings.Split(inner, ",")
		var arr []interface{}
		for _, p := range parts {
			p = strings.TrimSpace(p)
			p = strings.Trim(p, "\"'")
			arr = append(arr, p)
		}
		return arr
	}
	s = strings.Trim(s, "\"'")
	return s
}

func compileRust() bool {
	root := projectRoot()
	fmt.Printf("  %sCompiling Rust...%s\n", yellow, reset)
	cmd := exec.Command("cargo", "build")
	cmd.Dir = root
	cmd.Stdout = os.Stdout
	cmd.Stderr = os.Stderr
	if err := cmd.Run(); err != nil {
		fmt.Printf("  %s✗ Rust build failed%s\n", red, reset)
		return false
	}
	fmt.Printf("  %s✓ Rust build successful%s\n", green, reset)
	return true
}

func binaryPath() string {
	name := "proxycache"
	if runtime.GOOS == "windows" {
		name += ".exe"
	}
	return filepath.Join("target", "debug", name)
}

func printHelp() {
	fmt.Printf("  %s%sProxy control%s\n", bold, cyan, reset)
	fmt.Printf("    %srun%s       Start proxy (detached, survives CLI exit)\n", cyan, reset)
	fmt.Printf("    %sstatus%s    Check proxy process & API status\n", cyan, reset)
	fmt.Printf("    %sstop%s      Stop the proxy\n", cyan, reset)
	fmt.Printf("    %sreload%s    Stop → compile → start\n", cyan, reset)
	fmt.Printf("    %slogs%s      Show last 50 log lines\n", cyan, reset)
	fmt.Printf("    %sping%s      Quick connectivity check\n\n", cyan, reset)
	fmt.Printf("  %s%sConfig%s\n", bold, cyan, reset)
	fmt.Printf("    %sls%s        List server, modules & web\n", cyan, reset)
	fmt.Printf("    %stoggle%s    Toggle module on/off       %s(toggle rate_limiter)%s\n", cyan, reset, dim, reset)
	fmt.Printf("    %sedit%s      Edit server or module      %s(edit server, edit cache)%s\n\n", cyan, reset, dim, reset)
	fmt.Printf("  %s%sDevelopment%s\n", bold, cyan, reset)
	fmt.Printf("    %scompile%s   Build Rust + CLI & restart CLI\n", cyan, reset)
	fmt.Printf("    %sweb%s       Launch web dashboard\n", cyan, reset)
	fmt.Printf("    %sclear%s     Clear screen\n", cyan, reset)
	fmt.Printf("    %sexit%s      Exit CLI (proxy keeps running)\n", cyan, reset)
}
