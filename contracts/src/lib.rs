//! # Voting Contract
//!
//! A Soroban smart contract for creating proposals and casting votes.

#![no_std]

use soroban_sdk::{contract, contractimpl, contracttype, Address, Env, String, Symbol, Vec};

/// Maximum allowed length (in bytes) for a vote's `comment_hash`.
pub const MAX_COMMENT_HASH_LEN: u32 = 64;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Proposal {
    pub id: u64,
    pub creator: Address,
    pub title: String,
    pub description: String,
    pub yes_votes: u32,
    pub no_votes: u32,
    pub is_open: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VoteRecord {
    pub voter: Address,
    pub proposal_id: u64,
    pub support: bool,
    pub comment_hash: Option<String>,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Proposal(u64),
    Vote(u64, Address),
    ProposalCount,
}

#[contract]
pub struct VotingContract;

#[contractimpl]
impl VotingContract {
    /// Create a new proposal and return its id.
    pub fn create_proposal(
        env: Env,
        creator: Address,
        title: String,
        description: String,
    ) -> u64 {
        creator.require_auth();

        let mut count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .unwrap_or(0);
        count += 1;

        let proposal = Proposal {
            id: count,
            creator,
            title,
            description,
            yes_votes: 0,
            no_votes: 0,
            is_open: true,
        };

        env.storage()
            .persistent()
            .set(&DataKey::Proposal(count), &proposal);
        env.storage()
            .instance()
            .set(&DataKey::ProposalCount, &count);

        env.events()
            .publish((Symbol::new(&env, "proposal_created"),), count);

        count
    }

    /// Cast a vote on an open proposal.
    ///
    /// `comment_hash` is an optional hash of an off-chain comment. When
    /// provided it must not exceed [`MAX_COMMENT_HASH_LEN`] bytes.
    pub fn cast_vote(
        env: Env,
        voter: Address,
        proposal_id: u64,
        support: bool,
        comment_hash: Option<String>,
    ) {
        voter.require_auth();

        if let Some(ref hash) = comment_hash {
            if hash.len() > MAX_COMMENT_HASH_LEN {
                panic!("comment_hash exceeds MAX_COMMENT_HASH_LEN");
            }
        }

        let mut proposal: Proposal = env
            .storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .expect("proposal not found");

        if !proposal.is_open {
            panic!("proposal is closed");
        }

        let vote_key = DataKey::Vote(proposal_id, voter.clone());
        if env.storage().persistent().has(&vote_key) {
            panic!("already voted");
        }

        if support {
            proposal.yes_votes += 1;
        } else {
            proposal.no_votes += 1;
        }

        let record = VoteRecord {
            voter,
            proposal_id,
            support,
            comment_hash,
        };

        env.storage().persistent().set(&vote_key, &record);
        env.storage()
            .persistent()
            .set(&DataKey::Proposal(proposal_id), &proposal);

        env.events()
            .publish((Symbol::new(&env, "vote_cast"),), proposal_id);
    }

    /// Fetch a proposal by id.
    pub fn get_proposal(env: Env, proposal_id: u64) -> Proposal {
        env.storage()
            .persistent()
            .get(&DataKey::Proposal(proposal_id))
            .expect("proposal not found")
    }

    /// Fetch a vote record for a given proposal and voter.
    pub fn get_vote(env: Env, proposal_id: u64, voter: Address) -> VoteRecord {
        env.storage()
            .persistent()
            .get(&DataKey::Vote(proposal_id, voter))
            .expect("vote not found")
    }

    /// List all proposals.
    pub fn list_proposals(env: Env) -> Vec<Proposal> {
        let count: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ProposalCount)
            .unwrap_or(0);

        let mut proposals = Vec::new(&env);
        let mut i: u64 = 1;
        while i <= count {
            if let Some(p) = env
                .storage()
                .persistent()
                .get::<DataKey, Proposal>(&DataKey::Proposal(i))
            {
                proposals.push_back(p);
            }
            i += 1;
        }
        proposals
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::testutils::Address as _;
    use soroban_sdk::{Env, String};

    fn setup() -> (Env, VotingContractClient<'static>, Address, u64) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, VotingContract);
        let client = VotingContractClient::new(&env, &contract_id);
        let creator = Address::generate(&env);
        let proposal_id = client.create_proposal(
            &creator,
            &String::from_str(&env, "Title"),
            &String::from_str(&env, "Description"),
        );
        (env, client, creator, proposal_id)
    }

    #[test]
    fn cast_vote_with_none_comment_hash() {
        let (env, client, _creator, proposal_id) = setup();
        let voter = Address::generate(&env);

        client.cast_vote(&voter, &proposal_id, &true, &None);

        let record = client.get_vote(&proposal_id, &voter);
        assert_eq!(record.comment_hash, None);
        assert!(record.support);
    }

    #[test]
    fn cast_vote_with_valid_short_comment_hash() {
        let (env, client, _creator, proposal_id) = setup();
        let voter = Address::generate(&env);
        let hash = String::from_str(&env, "abc123");

        client.cast_vote(&voter, &proposal_id, &true, &Some(hash.clone()));

        let record = client.get_vote(&proposal_id, &voter);
        assert_eq!(record.comment_hash, Some(hash));
    }

    #[test]
    fn cast_vote_with_exact_max_comment_hash_len() {
        let (env, client, _creator, proposal_id) = setup();
        let voter = Address::generate(&env);
        let hash = String::from_str(
            &env,
            "0123456789012345678901234567890123456789012345678901234567890123",
        );
        assert_eq!(hash.len(), MAX_COMMENT_HASH_LEN);

        client.cast_vote(&voter, &proposal_id, &false, &Some(hash.clone()));

        let record = client.get_vote(&proposal_id, &voter);
        assert_eq!(record.comment_hash, Some(hash));
        assert!(!record.support);
    }

    #[test]
    #[should_panic(expected = "comment_hash exceeds MAX_COMMENT_HASH_LEN")]
    fn cast_vote_rejects_comment_hash_over_max_len() {
        let (env, client, _creator, proposal_id) = setup();
        let voter = Address::generate(&env);
        let hash = String::from_str(
            &env,
            "01234567890123456789012345678901234567890123456789012345678901234",
        );
        assert_eq!(hash.len(), MAX_COMMENT_HASH_LEN + 1);

        client.cast_vote(&voter, &proposal_id, &true, &Some(hash));
    }
}
