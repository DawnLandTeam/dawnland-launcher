package main

import (
	"log"

	"web-backend/database"
	"web-backend/handlers"
	"web-backend/tasks"

	"github.com/gin-gonic/gin"
	"github.com/joho/godotenv"
)

func main() {
	// Load environment variables from .env file.
	if err := godotenv.Load(); err != nil {
		log.Println("No .env file found, relying on system environment")
	}

	// Initialize database.
	database.InitDB()

	// Start background SLP ping task.
	tasks.StartPingTask()

	// Create Gin router with sensible defaults.
	r := gin.Default()

	// Health check endpoint.
	r.GET("/ping", func(c *gin.Context) {
		c.JSON(200, gin.H{"message": "pong"})
	})

	// Updater endpoint
	r.GET("/api/launcher/update/:target/:current_version", handlers.CheckUpdate)

	// CurseForge API proxy — injects server-side API key.
	r.Any("/api/curseforge/*path", handlers.CurseForgeProxy)

	// Microsoft Auth proxy
	ms := r.Group("/api/microsoft")
	{
		ms.POST("/devicecode", handlers.MsDeviceCode)
		ms.POST("/token", handlers.MsToken)
		ms.POST("/refresh", handlers.MsRefresh)
	}

	// Server CRUD endpoints.
	servers := r.Group("/api/servers")
	{
		servers.GET("", handlers.GetServers)
		servers.GET("/recommended", handlers.GetRecommendedServers)
		servers.GET("/pending", handlers.GetPendingServers)
		servers.GET("/filter-options", handlers.GetFilterOptions)
		servers.GET("/ip/:ip", handlers.GetServerByIP)
		servers.GET("/:id", handlers.GetServerByID)
		servers.POST("", handlers.CreateServer)
		servers.POST("/:id/approve", handlers.ApproveServer)
		servers.POST("/:id/reject", handlers.RejectServer)
		servers.PUT("/:id", handlers.UpdateServer)
		servers.DELETE("/:id", handlers.DeleteServer)
		// Pack file upload/download
		servers.POST("/:id/pack", handlers.UploadPackFile)
		servers.GET("/:id/pack", handlers.DownloadPackFile)
	}

	// Start the server on port 3030.
	log.Println("Starting Dawnland Web Backend on :3030")
	if err := r.Run(":3030"); err != nil {
		log.Fatal("Failed to start server: ", err)
	}
}
