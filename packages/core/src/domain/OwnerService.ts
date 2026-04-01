import { Owner } from '../models/types.js';
import { OwnerStorage } from '../storage/OwnerStorage.js';
import { TaskForgeError, ErrorCodes } from './errors.js';

export class OwnerService {
  constructor(private storage: OwnerStorage) {}

  async addOwner(name: string, type: 'human' | 'agent', description?: string): Promise<Owner> {
    const owner: Owner = { name, type, description, active: true };
    await this.storage.addOwner(owner);
    return owner;
  }

  async getOwner(name: string): Promise<Owner> {
    const owner = await this.storage.findOwner(name);
    if (!owner) throw new TaskForgeError(ErrorCodes.OWNER_NOT_FOUND, `Owner '${name}' not found`);
    return owner;
  }

  async listOwners(): Promise<Owner[]> {
    return this.storage.readAll();
  }

  async deactivateOwner(name: string): Promise<void> {
    const updated = await this.storage.updateOwner(name, { active: false });
    if (!updated) throw new TaskForgeError(ErrorCodes.OWNER_NOT_FOUND, `Owner '${name}' not found`);
  }
}
