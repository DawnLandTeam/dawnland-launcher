package models

import (
	"time"

	"gorm.io/gorm"
)

// Server represents a multiplayer Minecraft server entry.
type Server struct {
	ID             uint           `json:"id"`
	CreatedAt      time.Time      `json:"createdAt"`
	UpdatedAt      time.Time      `json:"updatedAt"`
	DeletedAt      gorm.DeletedAt `json:"deletedAt,omitempty" gorm:"index"`
	Name           string         `json:"name"`
	IP             string         `json:"ip" gorm:"uniqueIndex:idx_ip_port"`
	Port           int            `json:"port" gorm:"uniqueIndex:idx_ip_port"`
	Motd           string         `json:"motd"`
	Version        string         `json:"version"`
	LoaderType     string         `json:"loaderType"`             // Server loader: Vanilla, Fabric, Forge, Paper, etc.
	ServerType     string         `json:"serverType"`             // Server category: vanilla, modded, custom
	AuthType       string         `json:"authType"`               // Authentication type: offline, online
	PackFileName   string         `json:"packFileName,omitempty"` // Modpack ZIP file name (for modded/custom)
	PackFileSize   int64          `json:"packFileSize,omitempty"` // Modpack file size in bytes
	IconURL        string         `json:"iconUrl"`
	Email          string         `json:"email"` // Admin email for server management
	IsActive       bool           `json:"isActive" gorm:"default:false"`
	IsOnline       bool           `json:"isOnline" gorm:"default:false"` // Current online status
	CurrentPlayers int            `json:"currentPlayers" gorm:"default:0"`
	MaxPlayers     int            `json:"maxPlayers" gorm:"default:0"`
	Heat           int            `json:"heat" gorm:"default:0"` // Calculated heat value
}

// CreateServerInput defines the input for creating a new server.
type CreateServerInput struct {
	Name         string `json:"name" binding:"required"`
	IP           string `json:"ip" binding:"required"`
	Port         int    `json:"port" binding:"required"`
	Motd         string `json:"motd"`
	Version      string `json:"version"`
	LoaderType   string `json:"loaderType"`
	ServerType   string `json:"serverType"`   // vanilla, modded, custom
	AuthType     string `json:"authType"`     // offline, online
	PackFileName string `json:"packFileName"` // Modpack file name (set when pack is uploaded)
	IconURL      string `json:"iconUrl"`
	Email        string `json:"email" binding:"required,email"` // Email is required
}

// UpdateServerInput defines the input for updating a server (all fields optional).
type UpdateServerInput struct {
	Name         *string `json:"name"`
	IP           *string `json:"ip"`
	Port         *int    `json:"port"`
	Motd         *string `json:"motd"`
	Version      *string `json:"version"`
	LoaderType   *string `json:"loaderType"`
	ServerType   *string `json:"serverType"`
	AuthType     *string `json:"authType"`
	PackFileName *string `json:"packFileName"`
	IconURL      *string `json:"iconUrl"`
	Email        *string `json:"email"`
	IsActive     *bool   `json:"isActive"`
}
