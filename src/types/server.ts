/**
 * Server entity type definition.
 * Mirrors the Rust model in src-tauri/src/models/server.rs
 */

export interface Server {
  id: number;
  createdAt: string;
  updatedAt: string;
  name: string;
  ip: string;
  port: number;
  motd: string;
  version: string;
  iconUrl: string;
  isActive: boolean;
}

/**
 * Input type for creating a new server.
 */
export type CreateServerInput = Omit<Server, "id" | "createdAt" | "updatedAt">;

/**
 * Input type for updating a server.
 */
export type UpdateServerInput = Partial<Omit<Server, "id" | "createdAt" | "updatedAt">>;