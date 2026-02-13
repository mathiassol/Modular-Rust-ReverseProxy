// Web dashboard server
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"time"

	toml "github.com/pelletier/go-toml/v2"
)

var webPort = "8900"
var webRunning = false

func doWeb() {
	if webRunning {
		fmt.Printf("  %s! Web already running%s → %shttp://127.0.0.1:%s%s\n", yellow, reset, cyan, webPort, reset)
		return
	}

	root := projectRoot()
	webCfgPath := filepath.Join(root, ".proxycache-web.toml")

	// Check if web is enabled via virtual config
	if data, err := os.ReadFile(webCfgPath); err == nil {
		var wc map[string]interface{}
		if toml.Unmarshal(data, &wc) == nil {
			if e, ok := wc["enabled"].(bool); ok && !e {
				fmt.Printf("  %s✗ Web dashboard disabled. 'toggle web' to enable.%s\n", red, reset)
				return
			}
			if p, ok := wc["port"].(string); ok && p != "" {
				webPort = p
			}
		}
	}

	mux := http.NewServeMux()
	mux.HandleFunc("/api/config", webHandleConfig)
	mux.HandleFunc("/api/toggle/", webHandleToggle)
	mux.HandleFunc("/api/update/", webHandleUpdate)
	mux.HandleFunc("/api/proxy/status", webHandleProxyStatus)
	mux.HandleFunc("/api/proxy/start", webHandleProxyStart)
	mux.HandleFunc("/api/proxy/stop", webHandleProxyStop)
	mux.HandleFunc("/api/proxy/reload", webHandleProxyReload)
	mux.HandleFunc("/api/proxy/ping", webHandleProxyPing)
	mux.HandleFunc("/api/proxy/logs", webHandleProxyLogs)
	mux.HandleFunc("/api/proxy/compile", webHandleProxyCompile)
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html")
		w.Header().Set("Cache-Control", "no-cache, no-store, must-revalidate")
		w.Write([]byte(webIndexHTML))
	})

	ln, err := net.Listen("tcp", "127.0.0.1:"+webPort)
	if err != nil {
		fmt.Printf("  %s✗ Can't start web: %s%s\n", red, err, reset)
		return
	}
	webRunning = true
	url := fmt.Sprintf("http://127.0.0.1:%s", webPort)
	fmt.Printf("  %s✓ Web dashboard%s → %s%s%s\n", green, reset, cyan, url, reset)
	go http.Serve(ln, mux)
}

func isWebEnabled() bool {
	root := projectRoot()
	p := filepath.Join(root, ".proxycache-web.toml")
	data, err := os.ReadFile(p)
	if err != nil {
		return true
	}
	var wc map[string]interface{}
	if toml.Unmarshal(data, &wc) == nil {
		if e, ok := wc["enabled"].(bool); ok {
			return e
		}
	}
	return true
}

func toggleWeb() {
	root := projectRoot()
	p := filepath.Join(root, ".proxycache-web.toml")
	enabled := isWebEnabled()
	data := fmt.Sprintf("enabled = %v\nport = \"%s\"\n", !enabled, webPort)
	os.WriteFile(p, []byte(data), 0644)
	if !enabled {
		fmt.Printf("  %s✓ web enabled%s\n", green, reset)
	} else {
		fmt.Printf("  %s✗ web disabled%s\n", yellow, reset)
		webRunning = false
	}
}

func webJSON(w http.ResponseWriter, data interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.Header().Set("Access-Control-Allow-Origin", "*")
	json.NewEncoder(w).Encode(data)
}

func adminRequest(method, path string) (*http.Response, error) {
	req, err := http.NewRequest(method, fmt.Sprintf("http://%s%s", addr, path), nil)
	if err != nil {
		return nil, err
	}
	if apiKey != "" {
		req.Header.Set("X-API-Key", apiKey)
	}
	return client.Do(req)
}

func webErr(w http.ResponseWriter, code int, msg string) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	json.NewEncoder(w).Encode(map[string]string{"error": msg})
}

func webHandleConfig(w http.ResponseWriter, r *http.Request) {
	cfg, err := loadConfigTOML()
	if err != nil {
		webErr(w, 500, err.Error())
		return
	}

	type modInfo struct {
		Name     string                 `json:"name"`
		Enabled  bool                   `json:"enabled"`
		Settings map[string]interface{} `json:"settings"`
		IsServer bool                   `json:"is_server"`
	}
	var result []modInfo

	if srv, ok := cfg["server"].(map[string]interface{}); ok {
		result = append(result, modInfo{Name: "server", Enabled: true, Settings: srv, IsServer: true})
	}
	if mods := getModules(cfg); mods != nil {
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
			if e, ok := mod["enabled"].(bool); ok {
				enabled = e
			}
			settings := make(map[string]interface{})
			for k, v := range mod {
				if k != "enabled" {
					settings[k] = v
				}
			}
			result = append(result, modInfo{Name: name, Enabled: enabled, Settings: settings, IsServer: false})
		}
	}
	webJSON(w, result)
}

func webHandleToggle(w http.ResponseWriter, r *http.Request) {
	name := strings.TrimPrefix(r.URL.Path, "/api/toggle/")
	if name == "" || name == "server" {
		webErr(w, 400, "can't toggle server")
		return
	}
	cfg, err := loadConfigTOML()
	if err != nil {
		webErr(w, 500, err.Error())
		return
	}
	mods := getModules(cfg)
	if mods == nil {
		webErr(w, 500, "no modules")
		return
	}
	mod, ok := mods[name].(map[string]interface{})
	if !ok {
		webErr(w, 404, "not found")
		return
	}
	enabled := false
	if e, ok := mod["enabled"].(bool); ok {
		enabled = e
	}
	mod["enabled"] = !enabled
	mods[name] = mod
	cfg["modules"] = mods
	if err := saveConfigTOML(cfg); err != nil {
		webErr(w, 500, err.Error())
		return
	}
	webJSON(w, map[string]interface{}{"name": name, "enabled": !enabled})
}

func webHandleUpdate(w http.ResponseWriter, r *http.Request) {
	name := strings.TrimPrefix(r.URL.Path, "/api/update/")
	if name == "" {
		webErr(w, 400, "missing name")
		return
	}
	body, _ := io.ReadAll(r.Body)
	var updates map[string]interface{}
	if err := json.Unmarshal(body, &updates); err != nil {
		webErr(w, 400, "invalid json")
		return
	}
	cfg, err := loadConfigTOML()
	if err != nil {
		webErr(w, 500, err.Error())
		return
	}
	if name == "server" {
		srv, ok := cfg["server"].(map[string]interface{})
		if !ok {
			webErr(w, 500, "no server section")
			return
		}
		for k, v := range updates {
			srv[k] = coerceValue(srv[k], v)
		}
		cfg["server"] = srv
	} else {
		mods := getModules(cfg)
		if mods == nil {
			webErr(w, 500, "no modules")
			return
		}
		mod, ok := mods[name].(map[string]interface{})
		if !ok {
			webErr(w, 404, "not found")
			return
		}
		for k, v := range updates {
			mod[k] = coerceValue(mod[k], v)
		}
		mods[name] = mod
		cfg["modules"] = mods
	}
	if err := saveConfigTOML(cfg); err != nil {
		webErr(w, 500, err.Error())
		return
	}
	webJSON(w, map[string]string{"status": "saved"})
}

func coerceValue(existing, incoming interface{}) interface{} {
	switch v := incoming.(type) {
	case float64:
		if _, ok := existing.(int64); ok {
			return int64(v)
		}
		if v == float64(int64(v)) {
			return int64(v)
		}
		return v
	default:
		return incoming
	}
}

func webHandleProxyStatus(w http.ResponseWriter, r *http.Request) {
	root := projectRoot()
	pidFile := filepath.Join(root, ".proxycache.pid")
	result := map[string]interface{}{"process_running": false, "api_responding": false}
	if pid, err := readPID(pidFile); err == nil && isProcessRunning(pid) {
		result["process_running"] = true
		result["pid"] = pid
	}
	resp, err := adminRequest("GET", "/status")
	if err == nil {
		defer resp.Body.Close()
		body, _ := io.ReadAll(resp.Body)
		var apiData map[string]interface{}
		if json.Unmarshal(body, &apiData) == nil {
			result["api_responding"] = true
			result["process_running"] = true
			for k, v := range apiData {
				result[k] = v
			}
		}
	}
	webJSON(w, result)
}

func webHandleProxyStart(w http.ResponseWriter, r *http.Request) {
	root := projectRoot()
	pidFile := filepath.Join(root, ".proxycache.pid")
	if pid, err := readPID(pidFile); err == nil && isProcessRunning(pid) {
		webJSON(w, map[string]interface{}{"status": "already_running", "pid": pid})
		return
	}
	doRun()
	time.Sleep(500 * time.Millisecond)
	if pid, err := readPID(pidFile); err == nil {
		webJSON(w, map[string]interface{}{"status": "started", "pid": pid})
	} else {
		webJSON(w, map[string]string{"status": "started"})
	}
}

func webHandleProxyStop(w http.ResponseWriter, r *http.Request) {
	doStop()
	webJSON(w, map[string]string{"status": "stopped"})
}

func webHandleProxyReload(w http.ResponseWriter, r *http.Request) {
	go doReload()
	webJSON(w, map[string]string{"status": "reloading"})
}

func webHandleProxyPing(w http.ResponseWriter, r *http.Request) {
	start := time.Now()
	resp, err := adminRequest("GET", "/ping")
	elapsed := time.Since(start)
	if err != nil {
		webJSON(w, map[string]interface{}{"alive": false, "error": connErr(err)})
		return
	}
	resp.Body.Close()
	webJSON(w, map[string]interface{}{"alive": true, "latency_ms": elapsed.Milliseconds()})
}

func webHandleProxyLogs(w http.ResponseWriter, r *http.Request) {
	root := projectRoot()
	data, err := os.ReadFile(filepath.Join(root, ".proxycache.log"))
	if err != nil {
		webJSON(w, map[string]string{"logs": ""})
		return
	}
	lines := strings.Split(string(data), "\n")
	start := len(lines) - 200
	if start < 0 {
		start = 0
	}
	webJSON(w, map[string]string{"logs": strings.Join(lines[start:], "\n")})
}

func webHandleProxyCompile(w http.ResponseWriter, r *http.Request) {
	if compileRust() {
		webJSON(w, map[string]string{"status": "success"})
	} else {
		webErr(w, 500, "build failed")
	}
}
