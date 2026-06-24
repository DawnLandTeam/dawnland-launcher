export interface Account {
  id: string;
  username: string;
  accountType: "offline" | "microsoft" | "authlib";
  accessToken?: string;
  refreshToken?: string;
  textures?: string;
  authlibUrl?: string;
  authlibServerName?: string;
  authlibEmail?: string;
}

export interface AuthlibAuthResult {
  accessToken: string;
  clientToken: string;
  availableProfiles: any[] | null | undefined;
  authlibServerName?: string;
}

export interface LoginInitResponse {
  userCode: string;
  deviceCode: string;
  verificationUri: string;
  message: string;
}
