import fs from 'fs/promises';
import { Owner, OwnerSchema } from '../models/types.js';
import { ownersPath } from './paths.js';

export class OwnerStorage {
  constructor(private root: string) {}

  async readAll(): Promise<Owner[]> {
    try {
      const content = await fs.readFile(ownersPath(this.root), 'utf-8');
      const data = JSON.parse(content);
      return data.map((o: unknown) => OwnerSchema.parse(o));
    } catch {
      return [];
    }
  }

  async writeAll(owners: Owner[]): Promise<void> {
    await fs.writeFile(ownersPath(this.root), JSON.stringify(owners, null, 2), 'utf-8');
  }

  async findOwner(name: string): Promise<Owner | undefined> {
    const owners = await this.readAll();
    return owners.find(o => o.name === name);
  }

  async addOwner(owner: Owner): Promise<void> {
    const owners = await this.readAll();
    const existing = owners.findIndex(o => o.name === owner.name);
    if (existing >= 0) {
      owners[existing] = owner;
    } else {
      owners.push(owner);
    }
    await this.writeAll(owners);
  }

  async updateOwner(name: string, updates: Partial<Owner>): Promise<boolean> {
    const owners = await this.readAll();
    const idx = owners.findIndex(o => o.name === name);
    if (idx < 0) return false;
    owners[idx] = { ...owners[idx], ...updates };
    await this.writeAll(owners);
    return true;
  }
}
