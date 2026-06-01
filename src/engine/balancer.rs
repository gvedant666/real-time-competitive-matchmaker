use serde::Serialize;

#[derive(Clone, Serialize, Debug)]
pub struct MatchPlayer {
    pub uuid: String,
    pub mmr: u16,
}

#[derive(Clone, Serialize, Debug)]
pub struct MatchResponse {
    pub match_id: u64,
    pub team_a: Vec<MatchPlayer>,
    pub team_b: Vec<MatchPlayer>,
}

#[derive(Debug, Serialize)]
pub enum QueueEvent {
    MatchFound(MatchResponse),
    Timeout,
}

pub struct Match {
    pub team_a: Vec<MatchPlayer>,
    pub team_b: Vec<MatchPlayer>,
}

pub fn create_balanced_match(players: Vec<MatchPlayer>) -> Match {
    assert_eq!(players.len(), 10);

    let total_mmr: i32 = players.iter().map(|p| p.mmr as i32).sum();
    let mut best_mask = 0;
    let mut min_diff = i32::MAX;

    for mask in 0..1024_u16 {
        if mask.count_ones() == 5 {
            let mut team_a_sum = 0;
            for j in 0..10 {
                if (mask & (1 << j)) != 0 {
                    team_a_sum += players[j].mmr as i32;
                }
            }
            let team_b_sum = total_mmr - team_a_sum;
            let diff = (team_a_sum - team_b_sum).abs();

            if diff < min_diff {
                min_diff = diff;
                best_mask = mask;
            }
        }
    }

    let mut team_a = Vec::with_capacity(5);
    let mut team_b = Vec::with_capacity(5);

    for (j, player) in players.into_iter().enumerate() {
        if (best_mask & (1 << j)) != 0 {
            team_a.push(player);
        } else {
            team_b.push(player);
        }
    }

    Match { team_a, team_b }
}