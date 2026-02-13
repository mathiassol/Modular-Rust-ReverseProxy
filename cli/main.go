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
	case "mods":
		doMods()
	case "verify":
		doVerify()
	case "repair":
		doRepair()
	case "metrics":
		doMetrics()
	case "connections", "conns":
		doConnections()
	case "protocols", "proto":
		doProtocols()
	case "config":
		if len(args) > 0 {
			doEditSection(args[0])
		} else {
			doShowConfig()
		}
	case "tls":
		doTLS()
	case "server":
		doShowServer()
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
		fmt.Printf("  %s✗ Unknown: %s%s  (type 'help' for commands)\n", red, cmd, reset)
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
		var data map[string]interface{}
		if json.Unmarshal(body, &data) == nil {
			fmt.Printf("\n  %s%sOverview%s\n", bold, cyan, reset)
			fmt.Printf("  %s%s%s\n", dim, sep, reset)
			printStatusField("Listen", data["listen"])
			printStatusField("Backend", data["backend"])
			printStatusField("Scheme", data["scheme"])
			printStatusField("Protocols", data["protocols"])
			printStatusField("Uptime", data["uptime"])
			fmt.Printf("\n  %s%sTraffic%s\n", bold, cyan, reset)
			fmt.Printf("  %s%s%s\n", dim, sep, reset)
			printStatusField("Requests", data["requests_total"])
			printStatusField("OK", data["requests_ok"])
			printStatusField("Errors", data["requests_err"])
			printStatusField("Bytes In", formatBytes(data["bytes_in"]))
			printStatusField("Bytes Out", formatBytes(data["bytes_out"]))
			printStatusField("Avg Latency", fmt.Sprintf("%vms", data["avg_latency_ms"]))
			fmt.Printf("\n  %s%sResources%s\n", bold, cyan, reset)
			fmt.Printf("  %s%s%s\n", dim, sep, reset)
			printStatusField("Connections", fmt.Sprintf("%v / %v", data["active_connections"], data["max_connections"]))
			printStatusField("PID", data["pid"])
		}
	} else {
		fmt.Printf("  %s✗ API not responding%s\n", red, reset)
	}
}

func printStatusField(label string, value interface{}) {
	if value == nil {
		value = "—"
	}
	fmt.Printf("  %s%-16s%s %v\n", cyan, label, reset, value)
}

func formatBytes(v interface{}) string {
	if v == nil {
		return "0 B"
	}
	var b float64
	switch val := v.(type) {
	case float64:
		b = val
	case int64:
		b = float64(val)
	default:
		return fmt.Sprintf("%v", v)
	}
	if b < 1024 {
		return fmt.Sprintf("%.0f B", b)
	} else if b < 1024*1024 {
		return fmt.Sprintf("%.1f KB", b/1024)
	} else if b < 1024*1024*1024 {
		return fmt.Sprintf("%.1f MB", b/(1024*1024))
	}
	return fmt.Sprintf("%.2f GB", b/(1024*1024*1024))
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
		// Skip internal modules from CLI display
		if name == "proxy_core" {
			continue
		}
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

func doMetrics() {
	resp, err := adminRequest("GET", "/metrics")
	if err != nil {
		fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var data map[string]interface{}
	if json.Unmarshal(body, &data) != nil {
		fmt.Println(string(body))
		return
	}
	fmt.Printf("  %s%sRequests%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Total", data["requests_total"])
	printStatusField("OK", data["requests_ok"])
	printStatusField("Errors", data["requests_err"])
	fmt.Printf("\n  %s%sBandwidth%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Bytes In", formatBytes(data["bytes_in"]))
	printStatusField("Bytes Out", formatBytes(data["bytes_out"]))
	fmt.Printf("\n  %s%sLatency%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Avg (ms)", data["avg_latency_ms"])
	printStatusField("Max (ms)", data["latency_max_ms"])
	printStatusField("Sum (ms)", data["latency_sum_ms"])
	fmt.Printf("\n  %s%sConnections%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Active", data["active_connections"])
	printStatusField("Total", data["connections_total"])
	printStatusField("Pool Hits", data["pool_hits"])
	printStatusField("Pool Misses", data["pool_misses"])
	fmt.Printf("\n  %s%sCircuit Breaker%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Trips", data["cb_trips"])
	printStatusField("Rejects", data["cb_rejects"])
	fmt.Printf("\n  %s%sSystem%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Uptime", fmt.Sprintf("%vs", data["uptime_secs"]))
}

func doConnections() {
	resp, err := adminRequest("GET", "/connections")
	if err != nil {
		fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var data map[string]interface{}
	if json.Unmarshal(body, &data) != nil {
		fmt.Println(string(body))
		return
	}
	active := data["active"]
	max := data["max"]
	total := data["total_connections"]
	fmt.Printf("  %s%sConnections%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printStatusField("Active", active)
	printStatusField("Max Allowed", max)
	printStatusField("Total Served", total)
}

func doProtocols() {
	resp, err := adminRequest("GET", "/protocols")
	if err != nil {
		// Offline: read from config
		cfg, cfgErr := loadConfigTOML()
		if cfgErr != nil {
			fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
			return
		}
		srv, _ := cfg["server"].(map[string]interface{})
		fmt.Printf("  %s%sProtocols%s %s(from config, proxy not running)%s\n", bold, cyan, reset, dim, reset)
		fmt.Printf("  %s%s%s\n", dim, sep, reset)
		fmt.Printf("  %s✓ HTTP/1.1%s    always enabled\n", green, reset)
		h2, _ := srv["http2"].(bool)
		h3, _ := srv["http3"].(bool)
		tlsCert, _ := srv["tls_cert"].(string)
		tlsKey, _ := srv["tls_key"].(string)
		hasTLS := tlsCert != "" && tlsKey != ""
		if h2 && hasTLS {
			fmt.Printf("  %s✓ HTTP/2%s      ALPN via TLS\n", green, reset)
		} else if h2 {
			fmt.Printf("  %s● HTTP/2%s      enabled (needs TLS)\n", yellow, reset)
		} else {
			fmt.Printf("  %s✗ HTTP/2%s      disabled\n", red, reset)
		}
		if h3 && hasTLS {
			h3p, _ := srv["h3_port"].(int64)
			fmt.Printf("  %s✓ HTTP/3%s      QUIC port %d\n", green, reset, h3p)
		} else if h3 {
			fmt.Printf("  %s● HTTP/3%s      enabled (needs TLS)\n", yellow, reset)
		} else {
			fmt.Printf("  %s✗ HTTP/3%s      disabled\n", red, reset)
		}
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var data map[string]interface{}
	if json.Unmarshal(body, &data) != nil {
		fmt.Println(string(body))
		return
	}
	tls, _ := data["tls_enabled"].(bool)
	fmt.Printf("  %s%sProtocols%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	fmt.Printf("  %s✓ HTTP/1.1%s    always enabled\n", green, reset)
	if h2, ok := data["http2"].(map[string]interface{}); ok {
		if en, _ := h2["enabled"].(bool); en {
			fmt.Printf("  %s✓ HTTP/2%s      ALPN \"%s\"\n", green, reset, h2["alpn"])
		} else {
			reason := "disabled"
			if !tls {
				reason = "needs TLS"
			}
			fmt.Printf("  %s✗ HTTP/2%s      %s\n", red, reset, reason)
		}
	}
	if h3, ok := data["http3"].(map[string]interface{}); ok {
		if en, _ := h3["enabled"].(bool); en {
			fmt.Printf("  %s✓ HTTP/3%s      QUIC port %v\n", green, reset, h3["port"])
		} else {
			reason := "disabled"
			if !tls {
				reason = "needs TLS"
			}
			fmt.Printf("  %s✗ HTTP/3%s      %s\n", red, reset, reason)
		}
	}
	if tls {
		fmt.Printf("\n  %sTLS:%s enabled\n", cyan, reset)
	} else {
		fmt.Printf("\n  %sTLS:%s %snot configured%s\n", cyan, reset, dim, reset)
	}
}

func doTLS() {
	resp, err := adminRequest("GET", "/tls")
	if err != nil {
		// Offline
		cfg, cfgErr := loadConfigTOML()
		if cfgErr != nil {
			fmt.Printf("  %s✗ %s%s\n", red, connErr(err), reset)
			return
		}
		srv, _ := cfg["server"].(map[string]interface{})
		cert, _ := srv["tls_cert"].(string)
		key, _ := srv["tls_key"].(string)
		fmt.Printf("  %s%sTLS Configuration%s %s(from config)%s\n", bold, cyan, reset, dim, reset)
		fmt.Printf("  %s%s%s\n", dim, sep, reset)
		if cert == "" && key == "" {
			fmt.Printf("  %s✗ TLS not configured%s\n", red, reset)
			fmt.Printf("  %sSet tls_cert and tls_key in [server] to enable%s\n", dim, reset)
		} else {
			printStatusField("Cert", cert)
			printStatusField("Key", key)
		}
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var data map[string]interface{}
	if json.Unmarshal(body, &data) != nil {
		fmt.Println(string(body))
		return
	}
	fmt.Printf("  %s%sTLS Configuration%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	if en, _ := data["enabled"].(bool); en {
		fmt.Printf("  %s✓ TLS enabled%s\n", green, reset)
		printStatusField("Cert Path", data["cert_path"])
		printStatusField("Key Path", data["key_path"])
		certOk, _ := data["cert_exists"].(bool)
		keyOk, _ := data["key_exists"].(bool)
		if certOk {
			fmt.Printf("  %sCert File:%s %s✓ exists%s\n", cyan, reset, green, reset)
		} else {
			fmt.Printf("  %sCert File:%s %s✗ missing%s\n", cyan, reset, red, reset)
		}
		if keyOk {
			fmt.Printf("  %sKey File:%s  %s✓ exists%s\n", cyan, reset, green, reset)
		} else {
			fmt.Printf("  %sKey File:%s  %s✗ missing%s\n", cyan, reset, red, reset)
		}
		printStatusField("ALPN", data["alpn_protocols"])
		printStatusField("Session Cache", data["session_cache_size"])
	} else {
		fmt.Printf("  %s✗ TLS not configured%s\n", red, reset)
		fmt.Printf("  %sSet tls_cert and tls_key in [server] to enable%s\n", dim, reset)
	}
}

func doShowConfig() {
	// Try API first
	resp, err := adminRequest("GET", "/server")
	if err != nil {
		// Offline: read from config file
		cfg, cfgErr := loadConfigTOML()
		if cfgErr != nil {
			fmt.Printf("  %s✗ Can't read config: %s%s\n", red, cfgErr, reset)
			return
		}
		fmt.Printf("  %s%s[server]%s %s(from config.toml)%s\n", bold, cyan, reset, dim, reset)
		fmt.Printf("  %s%s%s\n", dim, sep, reset)
		if srv, ok := cfg["server"].(map[string]interface{}); ok {
			printSortedKV(srv)
		}
		fmt.Printf("\n  %s%s[modules]%s %s(from config.toml)%s\n", bold, cyan, reset, dim, reset)
		fmt.Printf("  %s%s%s\n", dim, sep, reset)
		if mods := getModules(cfg); mods != nil {
			names := sortedKeys(mods)
			for _, name := range names {
				// Skip internal modules from CLI display
				if name == "proxy_core" {
					continue
				}
				mod, ok := mods[name].(map[string]interface{})
				if !ok {
					continue
				}
				enabled := false
				if e, ok := mod["enabled"].(bool); ok {
					enabled = e
				}
				icon := red + "✗" + reset
				if enabled {
					icon = green + "✓" + reset
				}
				fmt.Printf("  %s %s%-16s%s", icon, cyan, name, reset)
				// Show key settings inline
				keys := sortedKeys(mod)
				parts := []string{}
				for _, k := range keys {
					if k == "enabled" {
						continue
					}
					parts = append(parts, fmt.Sprintf("%s=%v", k, mod[k]))
				}
				if len(parts) > 0 {
					fmt.Printf(" %s%s%s", dim, strings.Join(parts, ", "), reset)
				}
				fmt.Println()
			}
		}
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var data map[string]interface{}
	if json.Unmarshal(body, &data) != nil {
		fmt.Println(string(body))
		return
	}
	fmt.Printf("  %s%s[server]%s %s(live)%s\n", bold, cyan, reset, dim, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	printSortedKV(data)
}

func doShowServer() {
	doShowConfig()
}

func printSortedKV(m map[string]interface{}) {
	keys := sortedKeys(m)
	for _, k := range keys {
		v := m[k]
		switch val := v.(type) {
		case float64:
			if val == float64(int64(val)) {
				fmt.Printf("  %s%-20s%s %d\n", cyan, k, reset, int64(val))
			} else {
				fmt.Printf("  %s%-20s%s %g\n", cyan, k, reset, val)
			}
		case bool:
			if val {
				fmt.Printf("  %s%-20s%s %s%v%s\n", cyan, k, reset, green, val, reset)
			} else {
				fmt.Printf("  %s%-20s%s %s%v%s\n", cyan, k, reset, dim, val, reset)
			}
		default:
			fmt.Printf("  %s%-20s%s %v\n", cyan, k, reset, v)
		}
	}
}

func sortedKeys(m map[string]interface{}) []string {
	keys := make([]string, 0, len(m))
	for k := range m {
		keys = append(keys, k)
	}
	sort.Strings(keys)
	return keys
}

func printHelp() {
	fmt.Printf("  %s%sProxy Control%s\n", bold, cyan, reset)
	fmt.Printf("    %srun%s         Start proxy (detached)\n", cyan, reset)
	fmt.Printf("    %sstatus%s      Full proxy status + metrics summary\n", cyan, reset)
	fmt.Printf("    %sstop%s        Stop the proxy\n", cyan, reset)
	fmt.Printf("    %sreload%s      Stop → compile → start\n", cyan, reset)
	fmt.Printf("    %slogs%s        Show last 50 log lines\n", cyan, reset)
	fmt.Printf("    %sping%s        Quick connectivity check\n\n", cyan, reset)
	fmt.Printf("  %s%sMonitoring%s\n", bold, cyan, reset)
	fmt.Printf("    %smetrics%s     Full metrics (requests, latency, pool, CB)\n", cyan, reset)
	fmt.Printf("    %sconns%s       Active/max/total connections\n", cyan, reset)
	fmt.Printf("    %sprotocols%s   HTTP/1.1, HTTP/2, HTTP/3 status\n", cyan, reset)
	fmt.Printf("    %stls%s         TLS configuration and cert status\n\n", cyan, reset)
	fmt.Printf("  %s%sConfiguration%s\n", bold, cyan, reset)
	fmt.Printf("    %sconfig%s      Show full server + module config\n", cyan, reset)
	fmt.Printf("    %sls%s          List modules with on/off status\n", cyan, reset)
	fmt.Printf("    %stoggle%s      Toggle module on/off       %s(toggle rate_limiter)%s\n", cyan, reset, dim, reset)
	fmt.Printf("    %sedit%s        Edit server or module      %s(edit server, edit cache)%s\n", cyan, reset, dim, reset)
	fmt.Printf("    %sverify%s      Verify config.toml integrity\n", cyan, reset)
	fmt.Printf("    %srepair%s      Auto-repair config with missing defaults\n\n", cyan, reset)
	fmt.Printf("  %s%sModules%s\n", bold, cyan, reset)
	fmt.Printf("    %smods%s        List script (.pcmod) + Rust + imported modules\n\n", cyan, reset)
	fmt.Printf("  %s%sDevelopment%s\n", bold, cyan, reset)
	fmt.Printf("    %scompile%s     Build Rust + CLI & restart CLI\n", cyan, reset)
	fmt.Printf("    %sweb%s         Launch web dashboard\n", cyan, reset)
	fmt.Printf("    %sclear%s       Clear screen\n", cyan, reset)
	fmt.Printf("    %sexit%s        Exit CLI (proxy keeps running)\n", cyan, reset)
}

func doMods() {
	root := projectRoot()

	// List .pcmod files from mods/ directory
	modsDir := filepath.Join(root, "mods")
	entries, err := os.ReadDir(modsDir)

	fmt.Printf("  %s%sScript Modules (.pcmod)%s\n", bold, cyan, reset)
	fmt.Printf("  %s%-20s %-10s %s%s\n", dim, "NAME", "VERSION", "FILE", reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)

	if err != nil {
		fmt.Printf("  %sNo mods/ directory found%s\n", dim, reset)
	} else {
		found := false
		for _, e := range entries {
			if e.IsDir() || !strings.HasSuffix(e.Name(), ".pcmod") {
				continue
			}
			found = true
			data, err := os.ReadFile(filepath.Join(modsDir, e.Name()))
			if err != nil {
				fmt.Printf("  %-20s %s(error reading)%s\n", e.Name(), red, reset)
				continue
			}
			name, version := parsePcmod(string(data))
			fmt.Printf("  %-20s %-10s %s%s%s\n", name, version, dim, e.Name(), reset)
		}
		if !found {
			fmt.Printf("  %sNo .pcmod files found (check mods/examples/ for templates)%s\n", dim, reset)
		}
	}

	// List example .pcmod files
	exDir := filepath.Join(modsDir, "examples")
	exEntries, exErr := os.ReadDir(exDir)
	if exErr == nil && len(exEntries) > 0 {
		fmt.Printf("\n  %s%sExample Templates (mods/examples/)%s\n", bold, cyan, reset)
		fmt.Printf("  %s%s%s\n", dim, sep, reset)
		for _, e := range exEntries {
			if !strings.HasSuffix(e.Name(), ".pcmod") {
				continue
			}
			data, _ := os.ReadFile(filepath.Join(exDir, e.Name()))
			name, version := parsePcmod(string(data))
			fmt.Printf("  %-20s %-10s %s%s%s\n", name, version, dim, e.Name(), reset)
		}
		fmt.Printf("\n  %sCopy examples to mods/ to activate: copy mods\\examples\\*.pcmod mods\\%s\n", dim, reset)
	}

	// List Rust modules
	fmt.Printf("\n  %s%sRust Modules (compiled)%s\n", bold, cyan, reset)
	fmt.Printf("  %s%s%s\n", dim, sep, reset)
	srcDir := filepath.Join(root, "src", "modules")
	srcEntries, _ := os.ReadDir(srcDir)
	for _, e := range srcEntries {
		n := e.Name()
		if e.IsDir() || n == "mod.rs" || n == "helpers.rs" || !strings.HasSuffix(n, ".rs") {
			continue
		}
		name := strings.TrimSuffix(n, ".rs")
		fmt.Printf("  %-20s %s(built-in)%s\n", name, dim, reset)
	}

	// List imports
	impDir := filepath.Join(root, "imports")
	impEntries, impErr := os.ReadDir(impDir)
	if impErr == nil {
		hasImports := false
		for _, e := range impEntries {
			if strings.HasSuffix(e.Name(), ".rs") {
				if !hasImports {
					fmt.Printf("\n  %s%sImported Modules (imports/)%s\n", bold, cyan, reset)
					fmt.Printf("  %s%s%s\n", dim, sep, reset)
					hasImports = true
				}
				name := strings.TrimSuffix(e.Name(), ".rs")
				fmt.Printf("  %-20s %s(needs compile)%s\n", name, yellow, reset)
			}
		}
	}
}

func parsePcmod(content string) (name, version string) {
	name = "unknown"
	version = "?"
	for _, line := range strings.Split(content, "\n") {
		line = strings.TrimSpace(line)
		if strings.HasPrefix(line, "mod ") {
			name = strings.TrimSpace(strings.TrimPrefix(line, "mod "))
			name = strings.Trim(name, "\"")
		}
		if strings.HasPrefix(line, "version ") {
			version = strings.TrimSpace(strings.TrimPrefix(line, "version "))
			version = strings.Trim(version, "\"")
		}
	}
	return
}

func doVerify() {
	// Try API first (if proxy is running)
	resp, err := adminRequest("GET", "/config/verify")
	if err == nil {
		defer resp.Body.Close()
		body, _ := io.ReadAll(resp.Body)
		var result map[string]interface{}
		if json.Unmarshal(body, &result) == nil {
			ok, _ := result["ok"].(bool)
			if ok {
				fmt.Printf("  %s✓ Config is valid%s\n", green, reset)
			} else {
				fmt.Printf("  %s✗ Config issues found:%s\n", red, reset)
				if issues, ok := result["issues"].([]interface{}); ok {
					for _, issue := range issues {
						fmt.Printf("    %s• %v%s\n", yellow, issue, reset)
					}
				}
				if errMsg, ok := result["error"].(string); ok {
					fmt.Printf("    %s• %s%s\n", red, errMsg, reset)
				}
			}
			return
		}
	}

	// Offline verify: just check config.toml parse
	root := projectRoot()
	cfgPath := filepath.Join(root, "config.toml")
	data, err := os.ReadFile(cfgPath)
	if err != nil {
		fmt.Printf("  %s✗ Cannot read config.toml: %s%s\n", red, err, reset)
		return
	}
	var cfg map[string]interface{}
	if err := toml.Unmarshal(data, &cfg); err != nil {
		fmt.Printf("  %s✗ Parse error: %s%s\n", red, err, reset)
		return
	}

	issues := []string{}
	if _, ok := cfg["server"]; !ok {
		issues = append(issues, "missing [server] section")
	}
	if _, ok := cfg["modules"]; !ok {
		issues = append(issues, "missing [modules] section")
	}

	if len(issues) == 0 {
		fmt.Printf("  %s✓ Config is valid%s\n", green, reset)
	} else {
		fmt.Printf("  %s✗ Config issues found:%s\n", red, reset)
		for _, issue := range issues {
			fmt.Printf("    %s• %s%s\n", yellow, issue, reset)
		}
	}
}

func doRepair() {
	resp, err := adminRequest("POST", "/config/repair")
	if err != nil {
		fmt.Printf("  %s✗ Proxy not running. Repair requires running proxy (for module discovery).%s\n", red, reset)
		fmt.Printf("  %sTip: start the proxy with 'run', then try 'repair' again%s\n", dim, reset)
		return
	}
	defer resp.Body.Close()
	body, _ := io.ReadAll(resp.Body)
	var result map[string]interface{}
	if json.Unmarshal(body, &result) == nil {
		ok, _ := result["ok"].(bool)
		if ok {
			if fixes, ok := result["fixes"].([]interface{}); ok && len(fixes) > 0 {
				fmt.Printf("  %s✓ Config repaired:%s\n", green, reset)
				for _, fix := range fixes {
					fmt.Printf("    %s• %v%s\n", cyan, fix, reset)
				}
			} else {
				fmt.Printf("  %s✓ Config already valid, no repairs needed%s\n", green, reset)
			}
		} else {
			fmt.Printf("  %s✗ Repair failed%s\n", red, reset)
			printJSON(body)
		}
	}
}
