package database

import (
	"log"
	"os"
	"strings"

	"web-backend/models"

	"github.com/glebarez/sqlite"
	"gorm.io/driver/mysql"
	"gorm.io/gorm"
)

// DB is the global database instance shared across the application.
var DB *gorm.DB

// InitDB opens the SQLite or MySQL database and runs auto-migration for all models.
func InitDB() {
	var err error

	dbType := os.Getenv("DB_TYPE")

	if dbType == "mysql" {
		dsn := os.Getenv("DB_DSN")
		if dsn == "" {
			log.Fatal("DB_DSN must be set when DB_TYPE is mysql")
		}
		
		// Ensure parseTime=true is included to fix time.Time scanning issues
		if !strings.Contains(dsn, "parseTime=true") {
			if strings.Contains(dsn, "?") {
				dsn += "&parseTime=true"
			} else {
				dsn += "?parseTime=true"
			}
		}
		
		DB, err = gorm.Open(mysql.Open(dsn), &gorm.Config{})
		log.Println("Connecting to MySQL database...")
	} else {
		// Fallback to SQLite
		DB, err = gorm.Open(sqlite.Open("sqlite.db"), &gorm.Config{})
		log.Println("Connecting to SQLite database...")
	}

	if err != nil {
		log.Fatal("Failed to connect to database: ", err)
	}

	err = DB.AutoMigrate(&models.Server{})
	if err != nil {
		log.Fatal("Failed to migrate database: ", err)
	}

	log.Println("Database initialized and migrated successfully.")
}
