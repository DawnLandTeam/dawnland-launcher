package tasks

import (
	"bytes"
	"encoding/json"
	"fmt"
	"io"
	"log"
	"net"
	"time"

	"web-backend/database"
	"web-backend/models"
)

// StartPingTask starts a background goroutine that pings all active servers every 3 minutes.
func StartPingTask() {
	go func() {
		ticker := time.NewTicker(3 * time.Minute)
		defer ticker.Stop()

		// Run immediately on startup
		pingAllServers()

		for range ticker.C {
			pingAllServers()
		}
	}()
}

func pingAllServers() {
	log.Println("[PingTask] Starting server ping sweep...")

	var servers []models.Server
	if err := database.DB.Where("is_active = ?", true).Find(&servers).Error; err != nil {
		log.Println("[PingTask] Error fetching active servers:", err)
		return
	}

	for _, server := range servers {
		// Ping each server
		status, err := pingServer(server.IP, server.Port)

		isOnline := false
		currentPlayers := 0
		maxPlayers := 0
		heat := 0

		if err == nil && status != nil {
			isOnline = true
			currentPlayers = status.Players.Online
			maxPlayers = status.Players.Max
			heat = (currentPlayers * 10) + (maxPlayers * 1)
		}

		// Update database
		err = database.DB.Model(&server).Select("is_online", "current_players", "max_players", "heat").Updates(map[string]interface{}{
			"is_online":       isOnline,
			"current_players": currentPlayers,
			"max_players":     maxPlayers,
			"heat":            heat,
		}).Error
		if err != nil {
			log.Printf("[PingTask] Failed to update server %d: %v\n", server.ID, err)
		}
	}
	log.Println("[PingTask] Sweep completed.")
}

type SLPResponse struct {
	Players struct {
		Max    int `json:"max"`
		Online int `json:"online"`
	} `json:"players"`
}

// pingServer performs a minimal Minecraft 1.7+ Server List Ping.
func pingServer(ip string, port int) (*SLPResponse, error) {
	address := fmt.Sprintf("%s:%d", ip, port)
	conn, err := net.DialTimeout("tcp", address, 3*time.Second)
	if err != nil {
		return nil, err
	}
	defer conn.Close()
	conn.SetDeadline(time.Now().Add(3 * time.Second))

	// Write Handshake
	var buf bytes.Buffer
	writeVarInt(&buf, 0x00) // Packet ID
	writeVarInt(&buf, 47)   // Protocol Version
	writeString(&buf, ip)   // Server Address

	portBuf := []byte{byte(port >> 8), byte(port & 0xFF)}
	buf.Write(portBuf) // Server Port

	writeVarInt(&buf, 1) // Next State = 1 (status)

	// Send Packet Length + Packet
	var outBuf bytes.Buffer
	writeVarInt(&outBuf, int32(buf.Len()))
	outBuf.Write(buf.Bytes())
	conn.Write(outBuf.Bytes())

	// Write Request
	// Packet ID 0x00, empty payload
	conn.Write([]byte{0x01, 0x00}) // Length 1, Packet ID 0x00

	// Read Response
	// Packet Length (varint)
	length, err := readVarInt(conn)
	if err != nil || length <= 0 {
		return nil, fmt.Errorf("failed to read packet length")
	}

	// Packet ID (varint)
	packetID, err := readVarInt(conn)
	if err != nil || packetID != 0x00 {
		return nil, fmt.Errorf("invalid packet ID")
	}

	// JSON String Length (varint)
	jsonLen, err := readVarInt(conn)
	if err != nil || jsonLen <= 0 {
		return nil, fmt.Errorf("invalid json length")
	}

	// Read JSON String
	jsonBytes := make([]byte, jsonLen)
	_, err = io.ReadFull(conn, jsonBytes)
	if err != nil {
		return nil, err
	}

	var status SLPResponse
	if err := json.Unmarshal(jsonBytes, &status); err != nil {
		return nil, err
	}

	return &status, nil
}

// Helper to write VarInt
func writeVarInt(w io.Writer, val int32) {
	uval := uint32(val)
	for {
		temp := byte(uval & 0b01111111)
		uval >>= 7
		if uval != 0 {
			temp |= 0b10000000
		}
		w.Write([]byte{temp})
		if uval == 0 {
			break
		}
	}
}

// Helper to write String
func writeString(w io.Writer, s string) {
	writeVarInt(w, int32(len(s)))
	w.Write([]byte(s))
}

// Helper to read VarInt
func readVarInt(r io.Reader) (int32, error) {
	var num int32
	var shift uint
	buf := make([]byte, 1)

	for {
		_, err := r.Read(buf)
		if err != nil {
			return 0, err
		}
		b := buf[0]
		num |= int32(b&0b01111111) << shift
		if (b & 0b10000000) == 0 {
			break
		}
		shift += 7
		if shift >= 32 {
			return 0, fmt.Errorf("VarInt is too big")
		}
	}
	return num, nil
}
